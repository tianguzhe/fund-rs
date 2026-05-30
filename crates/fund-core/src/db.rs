use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use std::path::PathBuf;

// ── Input records (constructed by CLI commands) ────────────────────────

/// One fund's NAV on one day. Fund-dimension data: independent of holdings,
/// shared across a fund's lots, and safely backfillable.
#[derive(Debug, Clone)]
pub struct NavRecord {
    pub date: String,
    pub code: String,
    pub name: String,
    pub fund_type: Option<String>,
    pub nav: f64,
    pub acc_nav: Option<f64>,
    pub growth: f64,
}

/// One lot's daily snapshot. Carries enough to upsert `funds`, `nav_daily` and
/// `position_daily` in one pass. Market value is derived as `shares * nav`.
#[derive(Debug, Clone)]
pub struct PositionSnapshot {
    pub date: String,
    pub code: String,
    pub name: String,
    pub fund_type: Option<String>,
    pub channel: String,
    /// Purchase date, part of the lot key. '' when unknown.
    pub buy_date: String,
    pub shares: f64,
    pub cost_nav: f64,
    pub nav: f64,
    pub acc_nav: Option<f64>,
    pub growth: f64,
}

/// A cash movement to persist. `amount` is signed (see `holdings_config::CashFlow`).
#[derive(Debug, Clone)]
pub struct CashFlowInput {
    pub date: String,
    pub amount: f64,
    pub flow_type: String,
    pub code: Option<String>,
    pub note: Option<String>,
}

// ── Connection / schema ────────────────────────────────────────────────

fn db_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let dir = PathBuf::from(home).join(".fund-rs");
    std::fs::create_dir_all(&dir).ok();
    dir.join("portfolio.db")
}

/// Create the real-ledger schema (idempotent). Five tables:
/// `funds` (metadata), `nav_daily` (fund-dimension NAV), `position_daily`
/// (per-lot daily snapshot, keyed including channel so split lots stay
/// separate), `portfolio_daily` (per-day totals = the "total price"), and
/// `cash_flows` (signed cash ledger). `code`/`note` on `cash_flows` are
/// NOT NULL DEFAULT '' so the UNIQUE constraint dedupes reliably — SQLite
/// treats NULLs as distinct, which would let re-synced flows duplicate.
fn init_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "PRAGMA foreign_keys = ON;

        CREATE TABLE IF NOT EXISTS funds (
            code       TEXT PRIMARY KEY,
            name       TEXT NOT NULL,
            fund_type  TEXT,
            updated_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS nav_daily (
            date    TEXT NOT NULL,
            code    TEXT NOT NULL REFERENCES funds(code),
            nav     REAL NOT NULL,
            acc_nav REAL,
            growth  REAL,
            PRIMARY KEY (date, code)
        );

        CREATE TABLE IF NOT EXISTS position_daily (
            date         TEXT NOT NULL,
            code         TEXT NOT NULL REFERENCES funds(code),
            channel      TEXT NOT NULL DEFAULT '',
            buy_date     TEXT NOT NULL DEFAULT '',
            shares       REAL NOT NULL,
            cost_nav     REAL,
            market_value REAL NOT NULL,
            PRIMARY KEY (date, code, channel, buy_date)
        );

        CREATE TABLE IF NOT EXISTS portfolio_daily (
            date               TEXT PRIMARY KEY,
            total_market_value REAL NOT NULL,
            total_cash         REAL NOT NULL,
            total_assets       REAL NOT NULL,
            total_cost         REAL,
            total_pnl          REAL
        );

        CREATE TABLE IF NOT EXISTS cash_flows (
            id        INTEGER PRIMARY KEY AUTOINCREMENT,
            date      TEXT NOT NULL,
            amount    REAL NOT NULL,
            flow_type TEXT NOT NULL,
            code      TEXT NOT NULL DEFAULT '',
            note      TEXT NOT NULL DEFAULT '',
            UNIQUE (date, amount, flow_type, code, note)
        );",
    )?;
    Ok(())
}

/// True if the DB on disk still uses the pre-ledger schema (old
/// `portfolio_daily.holding` column, or any `daily_returns*` table). Opens a
/// short-lived connection so the caller can rename the file afterwards.
fn file_is_legacy(path: &PathBuf) -> Result<bool> {
    let conn =
        Connection::open(path).with_context(|| format!("打开数据库失败: {}", path.display()))?;
    let has_holding: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('portfolio_daily') WHERE name='holding'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let has_legacy_tables: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type='table' AND name IN ('daily_returns','daily_returns_legacy')",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    Ok(has_holding > 0 || has_legacy_tables > 0)
}

fn today_str() -> String {
    Connection::open_in_memory()
        .and_then(|c| c.query_row("SELECT date('now')", [], |r| r.get::<_, String>(0)))
        .unwrap_or_else(|_| "backup".to_string())
}

/// Open the ledger DB. If a legacy-schema file is found it is backed up to
/// `portfolio.db.legacy-<date>` and a fresh DB is created — the old data is
/// retired (per design) but never silently destroyed, since it concerns real
/// money figures.
fn open() -> Result<Connection> {
    let path = db_path();

    if path.exists() && file_is_legacy(&path)? {
        let mut backup = path.clone().into_os_string();
        backup.push(format!(".legacy-{}", today_str()));
        let backup = PathBuf::from(backup);
        std::fs::rename(&path, &backup)
            .with_context(|| format!("备份旧库失败: {}", backup.display()))?;
        eprintln!("✓ 旧库已备份为 {}，并重建新 schema", backup.display());
    }

    let conn =
        Connection::open(&path).with_context(|| format!("打开数据库失败: {}", path.display()))?;
    init_schema(&conn)?;
    Ok(conn)
}

// ── Write paths ─────────────────────────────────────────────────────────

fn upsert_fund(
    conn: &Connection,
    code: &str,
    name: &str,
    fund_type: &Option<String>,
) -> Result<()> {
    // Keep an existing fund_type when the caller passes None (e.g. backfill
    // knows the name but not the type) via COALESCE on the incoming value.
    conn.execute(
        "INSERT INTO funds (code, name, fund_type, updated_at)
         VALUES (?1, ?2, ?3, date('now'))
         ON CONFLICT(code) DO UPDATE SET
             name=excluded.name,
             fund_type=COALESCE(excluded.fund_type, funds.fund_type),
             updated_at=excluded.updated_at",
        params![code, name, fund_type],
    )?;
    Ok(())
}

fn upsert_nav(conn: &Connection, r: &NavRecord) -> Result<()> {
    conn.execute(
        "INSERT INTO nav_daily (date, code, nav, acc_nav, growth)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(date, code) DO UPDATE SET
             nav=excluded.nav, acc_nav=excluded.acc_nav, growth=excluded.growth",
        params![r.date, r.code, r.nav, r.acc_nav, r.growth],
    )?;
    Ok(())
}

/// Insert cash flows, deduping on the natural key via INSERT OR IGNORE.
/// None code/note are stored as '' so the UNIQUE constraint matches on repeat
/// syncs (the whole ledger is re-sent on every `--save`).
fn save_cash_flows_tx(conn: &Connection, flows: &[CashFlowInput]) -> Result<()> {
    for f in flows {
        conn.execute(
            "INSERT OR IGNORE INTO cash_flows (date, amount, flow_type, code, note)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                f.date,
                f.amount,
                f.flow_type,
                f.code.clone().unwrap_or_default(),
                f.note.clone().unwrap_or_default(),
            ],
        )?;
    }
    Ok(())
}

/// Running cash balance as of (and including) `date`.
fn cash_balance_asof(conn: &Connection, date: &str) -> Result<f64> {
    let bal: f64 = conn.query_row(
        "SELECT COALESCE(SUM(amount), 0.0) FROM cash_flows WHERE date <= ?1",
        params![date],
        |r| r.get(0),
    )?;
    Ok(bal)
}

/// Persist a daily snapshot: upserts funds/nav/position rows, syncs the cash
/// ledger, then writes a single complete `portfolio_daily` total.
///
/// All position/portfolio rows key on one `snapshot_date` (the latest NAV date
/// among the lots). This guarantees the daily total is ONE complete row even
/// when some funds' NAV lags a day — keying `portfolio_daily` on each lot's own
/// date would split the total across dates and double-count cash. `nav_daily`
/// still records each fund's real NAV date (it is queried per-code, never
/// joined on the snapshot date), so NAV history stays accurate.
fn save_snapshot_tx(
    conn: &Connection,
    records: &[PositionSnapshot],
    flows: &[CashFlowInput],
) -> Result<()> {
    save_cash_flows_tx(conn, flows)?;
    if records.is_empty() {
        return Ok(());
    }

    let snapshot_date = records.iter().map(|r| r.date.as_str()).max().unwrap_or_default();

    for r in records {
        upsert_fund(conn, &r.code, &r.name, &r.fund_type)?;
        upsert_nav(
            conn,
            &NavRecord {
                date: r.date.clone(),
                code: r.code.clone(),
                name: r.name.clone(),
                fund_type: r.fund_type.clone(),
                nav: r.nav,
                acc_nav: r.acc_nav,
                growth: r.growth,
            },
        )?;
        let mv = r.shares * r.nav;
        conn.execute(
            "INSERT INTO position_daily
                (date, code, channel, buy_date, shares, cost_nav, market_value)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(date, code, channel, buy_date) DO UPDATE SET
                 shares=excluded.shares,
                 cost_nav=excluded.cost_nav,
                 market_value=excluded.market_value",
            params![snapshot_date, r.code, r.channel, r.buy_date, r.shares, r.cost_nav, mv],
        )?;
    }

    let total_mv: f64 = records.iter().map(|r| r.shares * r.nav).sum();
    let total_cost: f64 = records.iter().map(|r| r.shares * r.cost_nav).sum();
    let cash = cash_balance_asof(conn, snapshot_date)?;
    conn.execute(
        "INSERT INTO portfolio_daily
            (date, total_market_value, total_cash, total_assets, total_cost, total_pnl)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(date) DO UPDATE SET
             total_market_value=excluded.total_market_value,
             total_cash=excluded.total_cash,
             total_assets=excluded.total_assets,
             total_cost=excluded.total_cost,
             total_pnl=excluded.total_pnl",
        params![snapshot_date, total_mv, cash, total_mv + cash, total_cost, total_mv - total_cost],
    )?;
    Ok(())
}

/// Save a daily portfolio snapshot (positions + cash ledger). Public entry
/// used by `fund portfolio --save`.
pub fn save_snapshot(records: &[PositionSnapshot], flows: &[CashFlowInput]) -> Result<()> {
    let mut conn = open()?;
    let tx = conn.transaction()?;
    save_snapshot_tx(&tx, records, flows)?;
    tx.commit()?;
    eprintln!(
        "✓ 已保存 {} 条持仓快照 + {} 条现金流水到 {}",
        records.len(),
        flows.len(),
        db_path().display()
    );
    Ok(())
}

/// Backfill fund-dimension NAV history only. Does NOT touch position/portfolio
/// tables — historical share counts are unknown, so deriving past market value
/// would re-introduce the very distortion this redesign removes.
pub fn save_nav(records: &[NavRecord]) -> Result<()> {
    let mut conn = open()?;
    let tx = conn.transaction()?;
    for r in records {
        upsert_fund(&tx, &r.code, &r.name, &r.fund_type)?;
        upsert_nav(&tx, r)?;
    }
    tx.commit()?;
    eprintln!("✓ 已回填 {} 条净值记录到 {}", records.len(), db_path().display());
    Ok(())
}

// ── Export (connected query) ─────────────────────────────────────────────

/// Export the full ledger as JSON via connected queries. Output reflects the
/// new schema: a per-day portfolio timeline (total price), per-fund NAV series
/// with latest position summary, and the cash ledger.
pub fn export_json() -> Result<serde_json::Value> {
    let conn = open()?;
    export_value(&conn)
}

fn export_value(conn: &Connection) -> Result<serde_json::Value> {
    use serde_json::json;

    // Timeline dates from the portfolio totals.
    let mut dates: Vec<String> = conn
        .prepare("SELECT date FROM portfolio_daily ORDER BY date ASC")?
        .query_map([], |r| r.get::<_, String>(0))?
        .filter_map(|r| r.ok())
        .collect();
    if dates.is_empty() {
        return Ok(json!({}));
    }
    dates.sort();
    dates.dedup();
    let date_idx: std::collections::HashMap<String, usize> =
        dates.iter().enumerate().map(|(i, d)| (d.clone(), i)).collect();
    let n = dates.len();

    // Portfolio timeline.
    let mut total_mv = vec![0.0f64; n];
    let mut total_cash = vec![0.0f64; n];
    let mut total_assets = vec![0.0f64; n];
    let mut total_pnl = vec![0.0f64; n];
    {
        let mut stmt = conn.prepare(
            "SELECT date, total_market_value, total_cash, total_assets, total_pnl
             FROM portfolio_daily",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, f64>(1)?,
                r.get::<_, f64>(2)?,
                r.get::<_, f64>(3)?,
                r.get::<_, Option<f64>>(4)?.unwrap_or(0.0),
            ))
        })?;
        for row in rows.flatten() {
            if let Some(&i) = date_idx.get(&row.0) {
                total_mv[i] = row.1;
                total_cash[i] = row.2;
                total_assets[i] = row.3;
                total_pnl[i] = row.4;
            }
        }
    }

    // Per-fund NAV series + latest position summary.
    let codes: Vec<(String, String, Option<String>)> = conn
        .prepare("SELECT code, name, fund_type FROM funds ORDER BY code")?
        .query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?))
        })?
        .filter_map(|r| r.ok())
        .collect();

    let mut funds = Vec::new();
    for (code, name, fund_type) in &codes {
        let mut navs: Vec<Option<f64>> = vec![None; n];
        {
            let mut stmt = conn.prepare("SELECT date, nav FROM nav_daily WHERE code = ?1")?;
            let rows = stmt
                .query_map(params![code], |r| Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?)))?;
            for row in rows.flatten() {
                if let Some(&i) = date_idx.get(&row.0) {
                    navs[i] = Some(row.1);
                }
            }
        }

        // Latest position across channels on the most recent date that has one.
        let latest: Option<(f64, f64, f64)> = conn
            .query_row(
                "SELECT COALESCE(SUM(shares),0), COALESCE(SUM(shares*cost_nav),0),
                        COALESCE(SUM(market_value),0)
                 FROM position_daily
                 WHERE code = ?1 AND date = (
                     SELECT MAX(date) FROM position_daily WHERE code = ?1
                 )",
                params![code],
                |r| Ok((r.get::<_, f64>(0)?, r.get::<_, f64>(1)?, r.get::<_, f64>(2)?)),
            )
            .ok()
            .filter(|t| t.0 != 0.0);

        let (shares, cost_value, market_value) = latest.unwrap_or((0.0, 0.0, 0.0));
        let hold_return_pct =
            if cost_value > 0.0 { Some((market_value / cost_value - 1.0) * 100.0) } else { None };

        funds.push(json!({
            "code": code,
            "name": name,
            "fund_type": fund_type,
            "navs": navs,
            "shares": shares,
            "cost_value": cost_value,
            "market_value": market_value,
            "hold_return_pct": hold_return_pct,
        }));
    }

    // Cash ledger.
    let cash_flows: Vec<serde_json::Value> = conn
        .prepare("SELECT date, amount, flow_type, code, note FROM cash_flows ORDER BY date, id")?
        .query_map([], |r| {
            Ok(json!({
                "date": r.get::<_, String>(0)?,
                "amount": r.get::<_, f64>(1)?,
                "flow_type": r.get::<_, String>(2)?,
                "code": r.get::<_, String>(3)?,
                "note": r.get::<_, String>(4)?,
            }))
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(json!({
        "generated_at": dates.last().cloned().unwrap_or_default(),
        "dates": dates,
        "funds": funds,
        "cash_flows": cash_flows,
        "portfolio": {
            "total_market_value": total_mv,
            "total_cash": total_cash,
            "total_assets": total_assets,
            "total_pnl": total_pnl,
        },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        conn
    }

    fn pos(
        date: &str,
        code: &str,
        channel: &str,
        shares: f64,
        cost_nav: f64,
        nav: f64,
    ) -> PositionSnapshot {
        pos_lot(date, code, channel, "", shares, cost_nav, nav)
    }

    #[allow(clippy::too_many_arguments)]
    fn pos_lot(
        date: &str,
        code: &str,
        channel: &str,
        buy_date: &str,
        shares: f64,
        cost_nav: f64,
        nav: f64,
    ) -> PositionSnapshot {
        PositionSnapshot {
            date: date.to_string(),
            code: code.to_string(),
            name: code.to_string(),
            fund_type: Some("债券型".to_string()),
            channel: channel.to_string(),
            buy_date: buy_date.to_string(),
            shares,
            cost_nav,
            nav,
            acc_nav: Some(nav),
            growth: 0.1,
        }
    }

    #[test]
    fn split_lots_stay_separate_and_totals_sum() {
        // Same fund, two channels -> two position_daily rows (not merged),
        // one nav_daily row, portfolio total = sum of market values.
        let conn = mem();
        let recs = vec![
            pos("2026-05-29", "000171", "招商", 120000.0, 1.485, 1.500),
            pos("2026-05-29", "000171", "天天基金", 117000.0, 1.496, 1.500),
        ];
        save_snapshot_tx(&conn, &recs, &[]).unwrap();

        let pos_rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM position_daily WHERE code='000171'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(pos_rows, 2, "split lots must not be merged");

        let nav_rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM nav_daily WHERE code='000171'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(nav_rows, 1, "one nav row per fund per date");

        let (mv, assets, pnl): (f64, f64, f64) = conn
            .query_row(
                "SELECT total_market_value, total_assets, total_pnl FROM portfolio_daily",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        let expect_mv = (120000.0 + 117000.0) * 1.500;
        let expect_cost = 120000.0 * 1.485 + 117000.0 * 1.496;
        assert!((mv - expect_mv).abs() < 1e-6);
        assert!((assets - expect_mv).abs() < 1e-6, "no cash -> assets == market value");
        assert!((pnl - (expect_mv - expect_cost)).abs() < 1e-6);
    }

    #[test]
    fn split_lots_same_channel_by_buy_date_stay_separate() {
        // Same fund AND same channel, two batches bought on different dates
        // (dollar-cost averaging) must each persist — buy_date is part of the
        // lot key, so they don't overwrite each other.
        let conn = mem();
        let recs = vec![
            pos_lot("2026-05-29", "000171", "招商", "2026-05-08", 4868.43, 2.052, 2.10),
            pos_lot("2026-05-29", "000171", "招商", "2026-05-11", 43944.33, 2.046, 2.10),
        ];
        save_snapshot_tx(&conn, &recs, &[]).unwrap();

        let rows: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM position_daily WHERE code='000171' AND channel='招商'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(rows, 2, "same-channel split lots must stay separate by buy_date");
    }

    #[test]
    fn portfolio_total_is_single_complete_row_across_lagged_navs() {
        // One fund's NAV lags a day. portfolio_daily must still be ONE row,
        // keyed on the latest date, summing ALL lots, with cash counted once
        // (regression: per-date grouping split the total and double-counted cash).
        let conn = mem();
        let recs = vec![
            pos("2026-05-29", "000171", "招商", 100000.0, 1.0, 1.2),
            pos("2026-05-28", "020359", "招商", 50000.0, 1.0, 1.1), // lagged NAV date
        ];
        let flows = vec![CashFlowInput {
            date: "2026-05-20".into(),
            amount: 10000.0,
            flow_type: "deposit".into(),
            code: None,
            note: None,
        }];
        save_snapshot_tx(&conn, &recs, &flows).unwrap();

        let rows: i64 =
            conn.query_row("SELECT COUNT(*) FROM portfolio_daily", [], |r| r.get(0)).unwrap();
        assert_eq!(rows, 1, "one complete portfolio row even with lagged NAV dates");

        let (date, mv, cash, assets): (String, f64, f64, f64) = conn
            .query_row(
                "SELECT date, total_market_value, total_cash, total_assets FROM portfolio_daily",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(date, "2026-05-29", "keyed on the latest NAV date");
        assert!((mv - (100000.0 * 1.2 + 50000.0 * 1.1)).abs() < 1e-6, "sums ALL lots");
        assert!((cash - 10000.0).abs() < 1e-6, "cash counted once, not per date");
        assert!((assets - (120000.0 + 55000.0 + 10000.0)).abs() < 1e-6);

        // Both lots land on the snapshot date for a complete same-day snapshot.
        let pos_rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM position_daily WHERE date='2026-05-29'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(pos_rows, 2, "both lots recorded on the snapshot date");
    }

    #[test]
    fn cash_balance_accumulates_and_closes_total_assets() {
        let conn = mem();
        let flows = vec![
            CashFlowInput {
                date: "2026-05-27".into(),
                amount: 89571.0,
                flow_type: "redeem".into(),
                code: Some("420002".into()),
                note: None,
            },
            CashFlowInput {
                date: "2026-05-28".into(),
                amount: -50000.0,
                flow_type: "subscribe".into(),
                code: None,
                note: None,
            },
        ];
        let recs = vec![pos("2026-05-29", "000171", "招商", 100000.0, 1.0, 1.2)];
        save_snapshot_tx(&conn, &recs, &flows).unwrap();

        let bal = cash_balance_asof(&conn, "2026-05-29").unwrap();
        assert!((bal - 39571.0).abs() < 1e-6, "89571 - 50000 = 39571");

        let (assets, cash): (f64, f64) = conn
            .query_row("SELECT total_assets, total_cash FROM portfolio_daily", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert!((cash - 39571.0).abs() < 1e-6);
        assert!((assets - (120000.0 + 39571.0)).abs() < 1e-6, "assets = mv + cash");
    }

    #[test]
    fn cash_flows_dedupe_on_resync() {
        // The whole ledger is re-sent on every save; identical flows must not
        // duplicate (UNIQUE relies on '' for missing code/note).
        let conn = mem();
        let flow = CashFlowInput {
            date: "2026-05-27".into(),
            amount: 89571.0,
            flow_type: "redeem".into(),
            code: None,
            note: None,
        };
        save_cash_flows_tx(&conn, std::slice::from_ref(&flow)).unwrap();
        save_cash_flows_tx(&conn, std::slice::from_ref(&flow)).unwrap();
        let count: i64 =
            conn.query_row("SELECT COUNT(*) FROM cash_flows", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 1, "re-synced identical flow must dedupe");
    }

    #[test]
    fn export_has_timeline_funds_and_cash() {
        let conn = mem();
        let recs = vec![pos("2026-05-29", "000171", "招商", 100000.0, 1.0, 1.2)];
        let flows = vec![CashFlowInput {
            date: "2026-05-27".into(),
            amount: 10000.0,
            flow_type: "deposit".into(),
            code: None,
            note: None,
        }];
        save_snapshot_tx(&conn, &recs, &flows).unwrap();

        let v = export_value(&conn).unwrap();
        assert_eq!(v["dates"].as_array().unwrap().len(), 1);
        assert_eq!(v["funds"].as_array().unwrap().len(), 1);
        assert_eq!(v["cash_flows"].as_array().unwrap().len(), 1);
        let f = &v["funds"][0];
        assert_eq!(f["code"], "000171");
        // hold_return = 1.2/1.0 - 1 = 20%
        assert!((f["hold_return_pct"].as_f64().unwrap() - 20.0).abs() < 1e-6);
        assert!((v["portfolio"]["total_assets"][0].as_f64().unwrap() - 130000.0).abs() < 1e-6);
    }
}
