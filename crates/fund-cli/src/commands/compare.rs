use anyhow::{Context, Result};
use fund_core::api::Client;
use fund_core::models::*;
use serde::Serialize;
use std::path::PathBuf;

#[derive(Serialize)]
struct FundCompareData {
    detail: FundDetail,
    periods: Vec<PeriodIncrease>,
    yearly_returns: Vec<PeriodIncrease>,
    monthly_returns: Vec<PeriodIncrease>,
    accumulated_return: Vec<AccumulatedReturn>,
    history: Vec<NetValuePoint>,
    risk_metrics: RiskMetrics,
    manager_info: Option<ManagerInfo>,
    manager_eval: Option<ManagerPerformance>,
    manager_char: Option<ManagerHoldingChar>,
    scores: FundScores,
}

#[derive(Serialize)]
struct RiskMetrics {
    annualized_return: f64,
    max_drawdown: f64,
    volatility: f64,
    sharpe_ratio: f64,
    positive_days: usize,
    negative_days: usize,
    monthly_win_rate: f64,
    excess_return: f64,
    data_points: usize,
}

#[derive(Serialize)]
struct FundScores {
    overall: u32,
    return_score: u32,
    risk_score: u32,
    stability_score: u32,
    fee_score: u32,
    scale_score: u32,
    manager_score: u32,
    style_score: u32,
}

#[derive(Serialize)]
struct CompareOutput {
    generated_at: String,
    fund_a: FundCompareData,
    fund_b: FundCompareData,
}

fn compute_risk_metrics(history: &[NetValuePoint], monthly_returns: &[PeriodIncrease], acc_return: &[AccumulatedReturn]) -> RiskMetrics {
    if history.len() < 2 {
        return RiskMetrics {
            annualized_return: 0.0,
            max_drawdown: 0.0,
            volatility: 0.0,
            sharpe_ratio: 0.0,
            positive_days: 0,
            negative_days: 0,
            monthly_win_rate: 0.0,
            excess_return: 0.0,
            data_points: 0,
        };
    }

    let mut navs: Vec<f64> = history.iter().map(|p| p.net_value).collect();
    navs.reverse();

    let mut peak = navs[0];
    let mut max_dd = 0.0f64;
    for nav in &navs {
        if *nav > peak { peak = *nav; }
        let dd = (peak - nav) / peak;
        if dd > max_dd { max_dd = dd; }
    }

    let daily_returns: Vec<f64> = navs.windows(2).map(|w| w[1] / w[0] - 1.0).collect();
    let n = daily_returns.len() as f64;
    let total_return = navs.last().unwrap() / navs[0];
    let days = navs.len() as f64;
    let annualized_return = (total_return.powf(250.0 / days) - 1.0) * 100.0;

    let avg_ret = daily_returns.iter().sum::<f64>() / n;
    let variance = daily_returns.iter().map(|r| (r - avg_ret).powi(2)).sum::<f64>() / n;
    let volatility = variance.sqrt() * (250.0_f64).sqrt() * 100.0;

    let sharpe = if volatility > 0.0 { (annualized_return - 2.0) / volatility } else { 0.0 };
    let positive = daily_returns.iter().filter(|r| **r > 0.0).count();
    let negative = daily_returns.iter().filter(|r| **r < 0.0).count();

    let monthly_win_rate = if !monthly_returns.is_empty() {
        let wins = monthly_returns.iter().filter(|m| m.return_rate > 0.0).count();
        wins as f64 / monthly_returns.len() as f64 * 100.0
    } else { 0.0 };

    let excess_return = if let (Some(first), Some(last)) = (acc_return.first(), acc_return.last()) {
        (last.fund_return - first.fund_return) - (last.bench_return - first.bench_return)
    } else { 0.0 };

    RiskMetrics { annualized_return, max_drawdown: max_dd * 100.0, volatility, sharpe_ratio: sharpe, positive_days: positive, negative_days: negative, monthly_win_rate, excess_return, data_points: history.len() }
}

fn score_return(periods: &[PeriodIncrease], metrics: &RiskMetrics) -> u32 {
    let year_ret = periods.iter().find(|p| p.title == "Last Year");
    let mut score = if let Some(yr) = year_ret {
        let rank_pct = if yr.total > 0 { yr.rank as f64 / yr.total as f64 * 100.0 } else { 50.0 };
        if rank_pct <= 10.0 { 95 } else if rank_pct <= 25.0 { 80 } else if rank_pct <= 50.0 { 65 } else { 50 }
    } else { 50 };
    if metrics.excess_return > 2.0 { score = (score + 10).min(95); }
    else if metrics.excess_return > 0.0 { score = (score + 5).min(90); }
    score
}

fn score_risk(metrics: &RiskMetrics, fund_type: &str) -> u32 {
    if fund_type.contains("债券") || fund_type.contains("债") {
        let dd_score = if metrics.max_drawdown < 0.5 { 95.0 } else if metrics.max_drawdown < 1.0 { 85.0 } else if metrics.max_drawdown < 2.0 { 70.0 } else { 50.0 };
        let vol_score = if metrics.volatility < 1.0 { 95.0 } else if metrics.volatility < 2.0 { 80.0 } else if metrics.volatility < 5.0 { 65.0 } else { 50.0 };
        (dd_score * 0.6 + vol_score * 0.4) as u32
    } else {
        let sharpe_score = if metrics.sharpe_ratio > 1.5 { 90.0 } else if metrics.sharpe_ratio > 1.0 { 75.0 } else if metrics.sharpe_ratio > 0.5 { 60.0 } else { 40.0 };
        let dd_score = if metrics.max_drawdown < 10.0 { 85.0 } else if metrics.max_drawdown < 20.0 { 70.0 } else { 50.0 };
        (sharpe_score * 0.5 + dd_score * 0.5) as u32
    }
}

fn score_stability(metrics: &RiskMetrics, yearly_returns: &[PeriodIncrease]) -> u32 {
    let daily_win = if metrics.data_points > 0 { metrics.positive_days as f64 / (metrics.positive_days + metrics.negative_days) as f64 * 100.0 } else { 50.0 };
    let monthly_win = metrics.monthly_win_rate;
    let yearly_score = if yearly_returns.len() >= 2 {
        if yearly_returns.iter().all(|y| y.return_rate > 0.0) { 90 } else { 65 }
    } else { 70 };
    let win_score = if daily_win >= 60.0 { 90 } else if daily_win >= 55.0 { 80 } else if daily_win >= 50.0 { 70 } else { 55 };
    let monthly_score = if monthly_win >= 80.0 { 90 } else if monthly_win >= 60.0 { 80 } else if monthly_win >= 50.0 { 70 } else { 55 };
    (win_score as f64 * 0.3 + monthly_score as f64 * 0.4 + yearly_score as f64 * 0.3) as u32
}

fn score_fees(detail: &FundDetail) -> u32 {
    let mgr: f64 = detail.mgr_fee.trim_end_matches('%').parse().unwrap_or(0.5);
    let trust: f64 = detail.trust_fee.trim_end_matches('%').parse().unwrap_or(0.1);
    let total = mgr + trust;
    if total <= 0.20 { 95 } else if total <= 0.35 { 80 } else if total <= 0.50 { 70 } else if total <= 1.00 { 55 } else { 40 }
}

fn score_scale(detail: &FundDetail) -> u32 {
    let scale: f64 = detail.scale.parse().unwrap_or(0.0) / 100_000_000.0;
    if scale >= 5.0 && scale <= 100.0 { 90 } else if scale >= 2.0 && scale <= 200.0 { 75 } else { 55 }
}

fn score_manager(eval: &Option<ManagerPerformance>) -> u32 {
    if let Some(e) = eval {
        let sharpe: f64 = e.sharpe_1y.parse().unwrap_or(0.0);
        let dd: f64 = e.max_drawdown_1y.parse().unwrap_or(100.0);
        let mut score = 70;
        if sharpe > 1.0 { score += 10; }
        if dd < 0.05 { score += 10; }
        score.min(95)
    } else { 60 }
}

fn score_style(char_data: &Option<ManagerHoldingChar>) -> u32 {
    if let Some(ch) = char_data {
        let stock_pos: f64 = ch.stock_position.parse().unwrap_or(0.0);
        if stock_pos <= 5.0 { 90 } else if stock_pos <= 15.0 { 75 } else { 55 }
    } else { 70 }
}

fn compute_scores(detail: &FundDetail, periods: &[PeriodIncrease], metrics: &RiskMetrics, yearly: &[PeriodIncrease], eval: &Option<ManagerPerformance>, char_data: &Option<ManagerHoldingChar>) -> FundScores {
    let return_score = score_return(periods, metrics);
    let risk_score = score_risk(metrics, &detail.fund_type);
    let stability_score = score_stability(metrics, yearly);
    let fee_score = score_fees(detail);
    let scale_score = score_scale(detail);
    let manager_score = score_manager(eval);
    let style_score = score_style(char_data);
    let is_bond = detail.fund_type.contains("债券") || detail.fund_type.contains("债");
    let overall = if is_bond {
        (return_score * 15 + risk_score * 30 + stability_score * 20 + fee_score * 15 + scale_score * 10 + manager_score * 5 + style_score * 5) / 100
    } else {
        (return_score * 25 + risk_score * 30 + stability_score * 15 + fee_score * 10 + scale_score * 10 + manager_score * 5 + style_score * 5) / 100
    };
    FundScores { overall, return_score, risk_score, stability_score, fee_score, scale_score, manager_score, style_score }
}

fn fetch_fund(client: &Client, code: &str) -> Result<FundCompareData> {
    let detail = client.get_fund_estimate(code)?;
    let periods = client.get_period_increase(code)?;
    let yearly_returns = client.get_yearly_returns(code).unwrap_or_default();
    let monthly_returns = client.get_monthly_returns(code).unwrap_or_default();
    let accumulated_return = client.get_accumulated_return(code, "ln", "000300").unwrap_or_default();
    let history = client.get_net_value_history(code, 250)?;
    let risk_metrics = compute_risk_metrics(&history, &monthly_returns, &accumulated_return);

    let managers = client.get_fund_managers(code).unwrap_or_default();
    let manager_id = managers.first().map(|m| m.manager_id.clone());

    let (manager_info, manager_eval, manager_char) = if let Some(mid) = &manager_id {
        let info = client.get_manager_info(mid).ok();
        let eval = client.get_manager_performance(mid).ok();
        let char = client.get_manager_holding_char(mid).ok();
        (info, eval, char)
    } else { (None, None, None) };

    let scores = compute_scores(&detail, &periods, &risk_metrics, &yearly_returns, &manager_eval, &manager_char);

    Ok(FundCompareData {
        detail,
        periods,
        yearly_returns,
        monthly_returns,
        accumulated_return,
        history,
        risk_metrics,
        manager_info,
        manager_eval,
        manager_char,
        scores,
    })
}

pub fn run(code_a: &str, code_b: &str, output: &PathBuf) -> Result<()> {
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent).context("Failed to create output directory")?;
    }

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
