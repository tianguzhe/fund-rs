use anyhow::{Context, Result};
use fund_core::api::Client;
use fund_core::models::*;
use fund_core::scoring::{self, RiskMetrics};
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

fn compute_scores(
    detail: &FundDetail,
    periods: &[PeriodIncrease],
    yearly: &[PeriodIncrease],
    metrics: &RiskMetrics,
    eval: &Option<ManagerPerformance>,
    char_data: &Option<ManagerHoldingChar>,
) -> FundScores {
    let return_score = scoring::score_return(periods, metrics);
    let risk_score = scoring::score_risk(metrics, &detail.fund_type);
    let stability_score = scoring::score_stability(metrics, yearly);
    let fee_score = scoring::score_fees(detail);
    let scale_score = scoring::score_scale(detail);
    let manager_score = scoring::score_manager(eval);
    let style_score = scoring::score_holding_style(char_data);

    let is_bond = detail.fund_type.contains("债券") || detail.fund_type.contains("债");
    let overall = if is_bond {
        (return_score * 15
            + risk_score * 30
            + stability_score * 20
            + fee_score * 15
            + scale_score * 10
            + manager_score * 5
            + style_score * 5)
            / 100
    } else {
        (return_score * 25
            + risk_score * 30
            + stability_score * 15
            + fee_score * 10
            + scale_score * 10
            + manager_score * 5
            + style_score * 5)
            / 100
    };

    FundScores {
        overall,
        return_score,
        risk_score,
        stability_score,
        fee_score,
        scale_score,
        manager_score,
        style_score,
    }
}

fn fetch_fund(client: &Client, code: &str) -> Result<FundCompareData> {
    // Batch 1: parallel fetches independent of each other
    let (detail_r, periods_r, yearly_r, monthly_r, history_r, managers_r) =
        std::thread::scope(|s| {
            let t1 = s.spawn(|| client.get_fund_estimate(code));
            let t2 = s.spawn(|| client.get_period_increase(code));
            let t3 = s.spawn(|| client.get_yearly_returns(code));
            let t4 = s.spawn(|| client.get_monthly_returns(code));
            let t5 = s.spawn(|| client.get_net_value_history(code, 250));
            let t6 = s.spawn(|| client.get_fund_managers(code));
            (
                t1.join().unwrap(),
                t2.join().unwrap(),
                t3.join().unwrap(),
                t4.join().unwrap(),
                t5.join().unwrap(),
                t6.join().unwrap(),
            )
        });

    let detail = detail_r?;
    let periods = periods_r?;
    let yearly_returns = yearly_r.unwrap_or_default();
    let monthly_returns = monthly_r.unwrap_or_default();
    let history = history_r.unwrap_or_default();
    let managers = managers_r.unwrap_or_default();

    // accumulated_return depends on benchmark derived from fund type
    let benchmark = scoring::select_benchmark(&detail.fund_type);
    let accumulated_return =
        client.get_accumulated_return(code, "ln", benchmark).unwrap_or_default();

    let risk_metrics =
        scoring::compute_risk_metrics(&history, &monthly_returns, &accumulated_return);

    // Batch 2: manager details in parallel (depends on manager_id from batch 1)
    let manager_id = managers.into_iter().next().map(|m| m.manager_id);
    let (manager_info, manager_eval, manager_char) = if let Some(mid) = &manager_id {
        std::thread::scope(|s| {
            let t1 = s.spawn(|| client.get_manager_info(mid));
            let t2 = s.spawn(|| client.get_manager_performance(mid));
            let t3 = s.spawn(|| client.get_manager_holding_char(mid));
            (t1.join().unwrap().ok(), t2.join().unwrap().ok(), t3.join().unwrap().ok())
        })
    } else {
        (None, None, None)
    };

    let scores = compute_scores(
        &detail,
        &periods,
        &yearly_returns,
        &risk_metrics,
        &manager_eval,
        &manager_char,
    );

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

    eprintln!("并发查询 {} 和 {} ...", code_a, code_b);

    // Fetch both funds in parallel
    let (result_a, result_b) = std::thread::scope(|s| {
        let ta = s.spawn(|| fetch_fund(&client, code_a));
        let tb = s.spawn(|| fetch_fund(&client, code_b));
        (ta.join().unwrap(), tb.join().unwrap())
    });

    let fund_a = result_a?;
    let fund_b = result_b?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let compare = CompareOutput { generated_at: format!("{}", now), fund_a, fund_b };

    let json = serde_json::to_string_pretty(&compare)?;
    std::fs::write(output, &json)?;
    eprintln!("✓ 对比数据已写入 {}", output.display());
    Ok(())
}
