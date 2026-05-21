use anyhow::{bail, Result};
use fund_core::api::Client;
use fund_core::db::{self, DailyRecord};
use fund_core::holdings::{date_days, fetch_all_histories, holdings, profit_amount};

/// 将 API 历史数据中 [from, to] 范围内的交易日批量写入 SQLite
pub fn run(client: &Client, from: &str, to: &str) -> Result<()> {
    let d_from = date_days(from).ok_or_else(|| anyhow::anyhow!("无效日期: {}", from))?;
    let d_to = date_days(to).ok_or_else(|| anyhow::anyhow!("无效日期: {}", to))?;
    if d_from > d_to {
        bail!("from ({}) 不能晚于 to ({})", from, to);
    }

    let hold = holdings()?;
    let all_points = fetch_all_histories(client, &hold);

    let mut records: Vec<DailyRecord> = Vec::new();

    for (h, points_opt) in hold.iter().zip(all_points.iter()) {
        let points = match points_opt {
            Some(p) if !p.is_empty() => p,
            _ => {
                eprintln!("⚠  {} 数据获取失败，跳过", h.code);
                continue;
            }
        };

        for point in points.iter() {
            let d = match date_days(&point.date) {
                Some(d) => d,
                None => continue,
            };
            if d < d_from || d > d_to {
                continue;
            }

            records.push(DailyRecord {
                date: point.date.clone(),
                fund_code: h.code.to_string(),
                fund_name: h.name.to_string(),
                fund_type: None,
                holding: h.amount,
                nav: Some(point.net_value),
                acc_nav: Some(point.acc_value),
                daily_pct: point.growth,
                daily_pnl: profit_amount(h.amount, point.growth),
            });
        }
    }

    if records.is_empty() {
        eprintln!("⚠  在 {} ~ {} 范围内未找到任何交易日数据", from, to);
        return Ok(());
    }

    let mut dates: Vec<&str> = records.iter().map(|r| r.date.as_str()).collect();
    dates.sort_unstable();
    dates.dedup();

    db::save_records(&records)?;
    println!("覆盖交易日: {}", dates.join(", "));
    Ok(())
}
