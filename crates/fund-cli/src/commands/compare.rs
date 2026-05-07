use anyhow::Result;
use fund_core::api::Client;
use fund_core::models::{NetValuePoint, PeriodIncrease};
use serde::Serialize;
use std::path::PathBuf;

// Output structs with clean field names (FundDetail uses API rename attributes)

#[derive(Serialize)]
struct FundInfo {
    code: String,
    name: String,
    full_name: String,
    fund_type: String,
    estab_date: String,
    company: String,
    manager: String,
    custodian: String,
    scale: String,
    risk_level: String,
    mgr_fee: String,
    trust_fee: String,
    sales_fee: String,
}

#[derive(Serialize)]
struct FundCompareData {
    detail: FundInfo,
    periods: Vec<PeriodIncrease>,
    history: Vec<NetValuePoint>,
    recent_30d_avg_growth: f64,
    recent_30d_volatility: f64,
    recent_30d_max_drawdown: f64,
    recent_30d_positive_days: usize,
    recent_30d_negative_days: usize,
    recent_30d_best_day: f64,
    recent_30d_worst_day: f64,
}

#[derive(Serialize)]
struct CompareOutput {
    generated_at: String,
    fund_a: FundCompareData,
    fund_b: FundCompareData,
}

fn compute_stats(points: &[NetValuePoint]) -> (f64, f64, f64, usize, usize, f64, f64) {
    if points.is_empty() {
        return (0.0, 0.0, 0.0, 0, 0, 0.0, 0.0);
    }

    let growths: Vec<f64> = points.iter().map(|p| p.growth).collect();
    let avg = growths.iter().sum::<f64>() / growths.len() as f64;
    let variance = growths.iter().map(|g| (g - avg).powi(2)).sum::<f64>() / growths.len() as f64;
    let volatility = variance.sqrt();

    let mut peak = points[0].net_value;
    let mut max_dd = 0.0f64;
    for p in points {
        if p.net_value > peak {
            peak = p.net_value;
        }
        let dd = (peak - p.net_value) / peak * 100.0;
        if dd > max_dd {
            max_dd = dd;
        }
    }

    let positive = growths.iter().filter(|g| **g > 0.0).count();
    let negative = growths.iter().filter(|g| **g < 0.0).count();
    let best = growths.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let worst = growths.iter().cloned().fold(f64::INFINITY, f64::min);

    (avg, volatility, max_dd, positive, negative, best, worst)
}

fn fetch_fund(client: &Client, code: &str) -> Result<FundCompareData> {
    let d = client.get_fund_estimate(code)?;
    let detail = FundInfo {
        code: d.code,
        name: d.name,
        full_name: d.full_name,
        fund_type: d.fund_type,
        estab_date: d.estab_date,
        company: d.company,
        manager: d.manager,
        custodian: d.custodian,
        scale: d.scale,
        risk_level: d.risk_level,
        mgr_fee: d.mgr_fee,
        trust_fee: d.trust_fee,
        sales_fee: d.sales_fee,
    };

    let periods = client.get_period_increase(code)?;
    let history = client.get_net_value_history(code, 30)?;
    let (avg, volatility, max_dd, positive, negative, best, worst) = compute_stats(&history);

    Ok(FundCompareData {
        detail,
        periods,
        history,
        recent_30d_avg_growth: avg,
        recent_30d_volatility: volatility,
        recent_30d_max_drawdown: max_dd,
        recent_30d_positive_days: positive,
        recent_30d_negative_days: negative,
        recent_30d_best_day: best,
        recent_30d_worst_day: worst,
    })
}

pub fn run(code_a: &str, code_b: &str, output: &PathBuf) -> Result<()> {
    let client = Client::new();

    eprintln!("查询 {} ...", code_a);
    let fund_a = fetch_fund(&client, code_a)?;
    eprintln!("查询 {} ...", code_b);
    let fund_b = fetch_fund(&client, code_b)?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let compare = CompareOutput {
        generated_at: format!("{}", now),
        fund_a,
        fund_b,
    };

    let json = serde_json::to_string_pretty(&compare)?;
    std::fs::write(output, &json)?;
    eprintln!("✓ 对比数据已写入 {}", output.display());
    Ok(())
}
