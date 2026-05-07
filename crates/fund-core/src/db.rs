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
    pub holding: f64,
    pub day_pct: f64,
    pub day_amount: f64,
    pub week_pct: f64,
    pub week_amount: f64,
    pub month_pct: f64,
    pub month_amount: f64,
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
        "CREATE TABLE IF NOT EXISTS daily_returns (
            date         TEXT NOT NULL,
            fund_code    TEXT NOT NULL,
            fund_name    TEXT NOT NULL,
            holding      REAL NOT NULL,
            day_pct      REAL NOT NULL,
            day_amount   REAL NOT NULL,
            week_pct     REAL NOT NULL,
            week_amount  REAL NOT NULL,
            month_pct    REAL NOT NULL,
            month_amount REAL NOT NULL,
            PRIMARY KEY (date, fund_code)
        );",
    )?;
    Ok(conn)
}

pub fn save_records(records: &[DailyRecord]) -> Result<()> {
    let mut conn = open()?;
    let tx = conn.transaction()?;
    for r in records {
        tx.execute(
            "INSERT OR REPLACE INTO daily_returns VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![
                r.date,
                r.fund_code,
                r.fund_name,
                r.holding,
                r.day_pct,
                r.day_amount,
                r.week_pct,
                r.week_amount,
                r.month_pct,
                r.month_amount
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
    holding: f64,
    weight: f64,
    day_pcts: Vec<Option<String>>,
    day_amounts: Vec<Option<String>>,
    week_pcts: Vec<Option<String>>,
    week_amounts: Vec<Option<String>>,
    month_pcts: Vec<Option<String>>,
    month_amounts: Vec<Option<String>>,
    cumulative_amounts: Vec<f64>,
}

#[derive(Serialize)]
struct TotalExport {
    day_pcts: Vec<String>,
    day_amounts: Vec<String>,
    cumulative_amounts: Vec<String>,
}

/// Single-query export: fetches all data in one SQL pass, builds in memory.
pub fn export_json() -> Result<serde_json::Value> {
    let conn = open()?;

    let mut stmt = conn.prepare(
        "SELECT date, fund_code, fund_name, holding,
                day_pct, day_amount, week_pct, week_amount, month_pct, month_amount
         FROM daily_returns
         ORDER BY fund_code, date ASC",
    )?;

    let rows: Vec<DailyRecord> = stmt
        .query_map([], |row| {
            Ok(DailyRecord {
                date: row.get(0)?,
                fund_code: row.get(1)?,
                fund_name: row.get(2)?,
                holding: row.get(3)?,
                day_pct: row.get(4)?,
                day_amount: row.get(5)?,
                week_pct: row.get(6)?,
                week_amount: row.get(7)?,
                month_pct: row.get(8)?,
                month_amount: row.get(9)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    if rows.is_empty() {
        return Ok(serde_json::json!({}));
    }

    // Extract sorted dates
    let mut dates: Vec<String> = rows.iter().map(|r| r.date.clone()).collect();
    dates.sort_unstable();
    dates.dedup();

    // Group records by fund_code (single-lookup per row)
    let mut fund_codes: Vec<String> = Vec::new();
    let mut fund_map: HashMap<String, Vec<&DailyRecord>> = HashMap::new();
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

    // Build fund exports
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
        let mut week_pcts: Vec<Option<String>> = vec![None; n];
        let mut week_amounts: Vec<Option<String>> = vec![None; n];
        let mut month_pcts: Vec<Option<String>> = vec![None; n];
        let mut month_amounts: Vec<Option<String>> = vec![None; n];
        let mut cumulative_amounts = vec![0.0f64; n];
        let mut cum_amt = 0.0f64;

        for r in records {
            if let Some(&idx) = date_index.get(r.date.as_str()) {
                day_pcts[idx] = Some(format!("{:.4}", r.day_pct));
                day_amounts[idx] = Some(format!("{:.2}", r.day_amount));
                week_pcts[idx] = Some(format!("{:.4}", r.week_pct));
                week_amounts[idx] = Some(format!("{:.2}", r.week_amount));
                month_pcts[idx] = Some(format!("{:.4}", r.month_pct));
                month_amounts[idx] = Some(format!("{:.2}", r.month_amount));
                cum_amt += r.day_amount;
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
            holding,
            weight,
            day_pcts,
            day_amounts,
            week_pcts,
            week_amounts,
            month_pcts,
            month_amounts,
            cumulative_amounts,
        });
    }

    // Build per-date aggregates in one pass
    let mut date_aggregates: HashMap<&str, (f64, f64)> = HashMap::new();
    for row in &rows {
        let entry = date_aggregates.entry(row.date.as_str()).or_insert((0.0, 0.0));
        entry.0 += row.day_amount;
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
