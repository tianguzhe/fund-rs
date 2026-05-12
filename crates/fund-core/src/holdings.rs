use crate::api::Client;
use crate::models::NetValuePoint;

pub struct Holding {
    pub code: &'static str,
    pub name: &'static str,
    pub amount: f64,
}

pub fn holdings() -> Vec<Holding> {
    vec![
        Holding { code: "420002", name: "天弘永利债A", amount: 363_219.0 },
        Holding { code: "020359", name: "东方红慧鑫C", amount: 167_963.0 },
        Holding { code: "020262", name: "平安鑫惠90天A", amount: 105_634.0 },
        Holding { code: "021282", name: "上银慧元A", amount: 52_202.0 },
        Holding { code: "016816", name: "兴业120天A", amount: 40_225.0 },
        Holding { code: "013791", name: "大成稳安C", amount: 30_330.0 },
        Holding { code: "000171", name: "易方达裕丰A", amount: 149_960.0 },
    ]
}

/// Profit amount for a single fund: holding * pct / 100
pub fn profit_amount(holding: f64, pct: f64) -> f64 {
    holding * pct / 100.0
}

/// Concurrently fetch net value history for all holdings.
/// Returns results in the same order as `holdings()`.
pub fn fetch_all_histories(client: &Client, hold: &[Holding]) -> Vec<Option<Vec<NetValuePoint>>> {
    std::thread::scope(|s| {
        let handles: Vec<_> = hold
            .iter()
            .map(|h| s.spawn(|| client.get_net_value_history(h.code, HISTORY_DAYS).ok()))
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
