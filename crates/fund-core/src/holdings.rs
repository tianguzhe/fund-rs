use crate::api::Client;
use crate::holdings_config;
use crate::models::NetValuePoint;
use anyhow::Result;

/// In-memory holding record. Fields are owned `String` because data comes
/// from a user-editable JSON file at runtime — `&'static str` no longer fits.
pub struct Holding {
    pub code: String,
    pub name: String,
    pub amount: f64,
    pub channel: Option<String>,
}

impl From<holdings_config::HoldingEntry> for Holding {
    fn from(e: holdings_config::HoldingEntry) -> Self {
        Self { code: e.code, name: e.name, amount: e.amount, channel: e.channel }
    }
}

/// Load holdings from the user's JSON config (see `holdings_config::config_path`).
pub fn holdings() -> Result<Vec<Holding>> {
    Ok(holdings_config::load()?.into_iter().map(Holding::from).collect())
}

/// Profit amount for a single fund: holding * pct / 100
pub fn profit_amount(holding: f64, pct: f64) -> f64 {
    holding * pct / 100.0
}

/// Map a fund's FTYPE (e.g. "债券型-混合二级", "混合型-偏债") to a 6-char asset
/// class label. Shared between `portfolio` and `holdings` so allocation rows
/// stay aligned across commands.
pub fn classify(ftype: &str) -> &'static str {
    if ftype.contains("货币") {
        "货币"
    } else if ftype.contains("债券") {
        "债券"
    } else if ftype.contains("QDII") {
        "QDII"
    } else if ftype.contains("指数") || ftype.contains("ETF") {
        "指数"
    } else if ftype.contains("股票") {
        "股票"
    } else if ftype.contains("混合") {
        "混合"
    } else if ftype.contains("FOF") {
        "FOF"
    } else if ftype.is_empty() {
        "未知"
    } else {
        "其他"
    }
}

/// Concurrently fetch net value history for all holdings.
/// Returns results in the same order as `holdings()`.
pub fn fetch_all_histories(client: &Client, hold: &[Holding]) -> Vec<Option<Vec<NetValuePoint>>> {
    std::thread::scope(|s| {
        let handles: Vec<_> = hold
            .iter()
            .map(|h| s.spawn(|| client.get_net_value_history(&h.code, HISTORY_DAYS).ok()))
            .collect();
        handles.into_iter().map(|t| t.join().unwrap()).collect()
    })
}

// 多取 5 天缓冲，覆盖节假日导致交易日不足的情况
pub const HISTORY_DAYS: i32 = 35;
pub const WEEK_DAYS: i64 = 7;
pub const MONTH_DAYS: i64 = 30;

pub fn date_days(date: &str) -> Option<i64> {
    let mut it = date.splitn(3, '-');
    let y: i64 = it.next()?.parse().ok()?;
    let m: i64 = it.next()?.parse().ok()?;
    let d: i64 = it.next()?.parse().ok()?;
    // 近似天数，误差 ±3 天，足以区分 7/30 日窗口内的最近交易日
    Some(y * 365 + m * 30 + d)
}

pub fn period_return(points: &[NetValuePoint], nav0: f64, d0: i64, window: i64) -> f64 {
    let target = d0 - window;
    for p in points.iter().skip(1) {
        if let Some(d) = date_days(&p.date) {
            if d <= target && p.net_value != 0.0 {
                return (nav0 - p.net_value) / p.net_value * 100.0;
            }
        }
    }
    if let Some(last) = points.last() {
        if last.net_value != 0.0 {
            return (nav0 - last.net_value) / last.net_value * 100.0;
        }
    }
    0.0
}
