use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use serde::Serialize;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug)]
pub struct DailyRecord {
    pub date: String,
    pub fund_code: String,
    pub fund_name: String,
    pub fund_type: Option<String>,
    pub holding: f64,
    pub nav: Option<f64>,
    pub acc_nav: Option<f64>,
    pub daily_pct: f64,
    pub daily_pnl: f64,
}

fn db_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let dir = PathBuf::from(home).join(".fund-rs");
    std::fs::create_dir_all(&dir).ok();
    dir.join("portfolio.db")
}

fn open() -> Result<Connection> {
    let path = db_path();
    let conn =
        Connection::open(&path).with_context(|| format!("打开数据库失败: {}", path.display()))?;

    conn.execute_batch(
        "PRAGMA foreign_keys = ON;

        CREATE TABLE IF NOT EXISTS funds (
            code       TEXT PRIMARY KEY,
            name       TEXT NOT NULL,
            fund_type  TEXT,
            updated_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS portfolio_daily (
            date      TEXT NOT NULL,
            code      TEXT NOT NULL REFERENCES funds(code),
            holding   REAL NOT NULL,
            nav       REAL,
            acc_nav   REAL,
            daily_pct REAL NOT NULL,
            daily_pnl REAL NOT NULL,
            PRIMARY KEY (date, code)
        );",
    )?;

    maybe_migrate(&conn)?;

    Ok(conn)
}

/// One-time migration from legacy `daily_returns` table to the new schema.
/// Renames old table to `daily_returns_legacy` after successful migration.
fn maybe_migrate(conn: &Connection) -> Result<()> {
    let legacy_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='daily_returns'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0)
        > 0;

    if !legacy_exists {
        return Ok(());
    }

    // Migrate data in a transaction, then rename the old table outside it.
    conn.execute_batch(
        "BEGIN;
        INSERT OR IGNORE INTO funds (code, name, updated_at)
            SELECT fund_code, fund_name, MAX(date)
            FROM daily_returns
            GROUP BY fund_code;
        INSERT OR IGNORE INTO portfolio_daily (date, code, holding, daily_pct, daily_pnl)
            SELECT date, fund_code, holding, day_pct, day_amount
            FROM daily_returns;
        COMMIT;",
    )
    .context("迁移旧数据失败")?;

    // Rename outside transaction — DDL in SQLite is safe but cleaner this way.
    conn.execute_batch("ALTER TABLE daily_returns RENAME TO daily_returns_legacy;")
        .context("重命名旧表失败")?;

    eprintln!("✓ 已迁移旧数据到新表，原表保留为 daily_returns_legacy");
    Ok(())
}

pub fn save_records(records: &[DailyRecord]) -> Result<()> {
    let mut conn = open()?;
    let tx = conn.transaction()?;

    for r in records {
        tx.execute(
            "INSERT OR REPLACE INTO funds (code, name, fund_type, updated_at)
             VALUES (?1, ?2, ?3, date('now'))",
            params![r.fund_code, r.fund_name, r.fund_type],
        )?;

        tx.execute(
            "INSERT OR REPLACE INTO portfolio_daily
                (date, code, holding, nav, acc_nav, daily_pct, daily_pnl)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                r.date,
                r.fund_code,
                r.holding,
                r.nav,
                r.acc_nav,
                r.daily_pct,
                r.daily_pnl
            ],
        )?;
    }

    tx.commit()?;
    eprintln!("✓ 已保存 {} 条记录到 {}", records.len(), db_path().display());
    Ok(())
}

// ── Export types (serde-based) ─────────────────────────────────────────

#[derive(Serialize)]
struct ExportData {
    generated_at: String,
    dates: Vec<String>,
    funds: Vec<FundExport>,
    total: TotalExport,
}

#[derive(Serialize)]
struct FundExport {
    code: String,
    name: String,
    fund_type: Option<String>,
    holding: f64,
    weight: f64,
    day_pcts: Vec<Option<String>>,
    day_amounts: Vec<Option<String>>,
    cumulative_amounts: Vec<f64>,
}

#[derive(Serialize)]
struct TotalExport {
    day_pcts: Vec<String>,
    day_amounts: Vec<String>,
    cumulative_amounts: Vec<String>,
}

/// Single-query export: fetches all portfolio data, builds timeline in memory.
pub fn export_json() -> Result<serde_json::Value> {
    let conn = open()?;

    #[derive(Debug)]
    struct ExportRow {
        date: String,
        fund_code: String,
        fund_name: String,
        fund_type: Option<String>,
        holding: f64,
        daily_pct: f64,
        daily_pnl: f64,
    }

    let mut stmt = conn.prepare(
        "SELECT p.date, p.code, f.name, f.fund_type, p.holding, p.daily_pct, p.daily_pnl
         FROM portfolio_daily p
         JOIN funds f ON f.code = p.code
         ORDER BY p.code, p.date ASC",
    )?;

    let rows: Vec<ExportRow> = stmt
        .query_map([], |row| {
            Ok(ExportRow {
                date: row.get(0)?,
                fund_code: row.get(1)?,
                fund_name: row.get(2)?,
                fund_type: row.get(3)?,
                holding: row.get(4)?,
                daily_pct: row.get(5)?,
                daily_pnl: row.get(6)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    if rows.is_empty() {
        return Ok(serde_json::json!({}));
    }

    let mut dates: Vec<String> = rows.iter().map(|r| r.date.clone()).collect();
    dates.sort_unstable();
    dates.dedup();

    let mut fund_codes: Vec<String> = Vec::new();
    let mut fund_map: HashMap<String, Vec<&ExportRow>> = HashMap::new();
    for row in &rows {
        match fund_map.entry(row.fund_code.clone()) {
            Entry::Vacant(e) => {
                fund_codes.push(row.fund_code.clone());
                e.insert(vec![row]);
            }
            Entry::Occupied(mut e) => {
                e.get_mut().push(row);
            }
        }
    }

    let total_holding: f64 = fund_map.values().map(|recs| recs[0].holding).sum();

    let date_index: HashMap<&str, usize> =
        dates.iter().enumerate().map(|(i, d)| (d.as_str(), i)).collect();

    let mut funds = Vec::new();
    for code in &fund_codes {
        let records = &fund_map[code];
        let holding = records[0].holding;
        let weight = holding / total_holding * 100.0;
        let n = dates.len();

        let mut day_pcts: Vec<Option<String>> = vec![None; n];
        let mut day_amounts: Vec<Option<String>> = vec![None; n];
        let mut cumulative_amounts = vec![0.0f64; n];
        let mut cum_amt = 0.0f64;

        for r in records {
            if let Some(&idx) = date_index.get(r.date.as_str()) {
                day_pcts[idx] = Some(format!("{:.4}", r.daily_pct));
                day_amounts[idx] = Some(format!("{:.2}", r.daily_pnl));
                cum_amt += r.daily_pnl;
                cumulative_amounts[idx] = cum_amt;
            }
        }
        // Forward-fill cumulative for null gaps
        for i in 1..n {
            if cumulative_amounts[i] == 0.0 && day_amounts[i].is_none() {
                cumulative_amounts[i] = cumulative_amounts[i - 1];
            }
        }

        funds.push(FundExport {
            code: code.clone(),
            name: records[0].fund_name.clone(),
            fund_type: records[0].fund_type.clone(),
            holding,
            weight,
            day_pcts,
            day_amounts,
            cumulative_amounts,
        });
    }

    let mut date_aggregates: HashMap<&str, (f64, f64)> = HashMap::new();
    for row in &rows {
        let entry = date_aggregates.entry(row.date.as_str()).or_insert((0.0, 0.0));
        entry.0 += row.daily_pnl;
        entry.1 += row.holding;
    }

    let mut total_day_pcts = Vec::with_capacity(dates.len());
    let mut total_day_amounts = Vec::with_capacity(dates.len());
    let mut total_cumulative = Vec::with_capacity(dates.len());
    let mut cum = 0.0f64;

    for date in &dates {
        let (sum_amt, sum_holding) =
            date_aggregates.get(date.as_str()).copied().unwrap_or((0.0, total_holding));
        let pct = if sum_holding > 0.0 { sum_amt / sum_holding * 100.0 } else { 0.0 };
        cum += sum_amt;
        total_day_pcts.push(format!("{:.4}", pct));
        total_day_amounts.push(format!("{:.2}", sum_amt));
        total_cumulative.push(format!("{:.2}", cum));
    }

    let export = ExportData {
        generated_at: dates.last().cloned().unwrap_or_default(),
        dates,
        funds,
        total: TotalExport {
            day_pcts: total_day_pcts,
            day_amounts: total_day_amounts,
            cumulative_amounts: total_cumulative,
        },
    };

    serde_json::to_value(&export).context("JSON serialization failed")
}
