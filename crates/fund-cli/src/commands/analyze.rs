use anyhow::Result;
use fund_core::api::Client;
use fund_core::f10::FeeRules;
use fund_core::models::*;
use fund_core::scoring::{self, RiskMetrics};
use owo_colors::OwoColorize;
use serde::Serialize;

#[derive(Serialize)]
pub struct FundAnalysis {
    pub detail: FundDetail,
    pub periods: Vec<PeriodIncrease>,
    pub yearly_returns: Vec<PeriodIncrease>,
    pub monthly_returns: Vec<PeriodIncrease>,
    pub accumulated_return: Vec<AccumulatedReturn>,
    pub risk_metrics: RiskMetrics,
    pub fee_rules: Option<FeeRules>,
    pub manager_eval: Option<ManagerPerformance>,
    pub manager_char: Option<ManagerHoldingChar>,
    pub manager_info: Option<ManagerInfo>,
}

pub fn run(client: &Client, code: &str, json: bool) -> Result<()> {
    eprintln!("查询基金 {} ...", code);

    // Batch 1: 6 independent requests in parallel
    let (detail_r, periods_r, yearly_r, monthly_r, nav_trend_r, managers_r) =
        std::thread::scope(|s| {
            let t1 = s.spawn(|| client.get_fund_estimate(code));
            let t2 = s.spawn(|| client.get_period_increase(code));
            let t3 = s.spawn(|| client.get_yearly_returns(code));
            let t4 = s.spawn(|| client.get_monthly_returns(code));
            // 3-year window covers at least one full market cycle for risk metrics.
            let t5 = s.spawn(|| client.get_nav_trend(code, "3n", 500));
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
    let nav_trend = nav_trend_r.unwrap_or_default();
    let managers = managers_r.unwrap_or_default();
    let fee_rules = fund_core::f10::get_fee_rules(code).ok();

    // Use INDEXCODE from fund detail for index/ETF funds; fallback to type-based selection.
    let benchmark = scoring::select_benchmark(&detail.fund_type, &detail.index_code);
    let accumulated_return =
        client.get_accumulated_return(code, "ln", &benchmark).unwrap_or_default();

    let risk_metrics =
        scoring::compute_risk_metrics(&nav_trend, &monthly_returns, &accumulated_return);

    // Batch 2: manager details in parallel (depends on manager_id from batch 1)
    let manager_id = managers.into_iter().next().map(|m| m.manager_id);
    let (manager_info, manager_eval, manager_char) = if let Some(mid) = &manager_id {
        eprintln!("查询经理 {} ...", mid);
        std::thread::scope(|s| {
            let t1 = s.spawn(|| client.get_manager_info(mid));
            let t2 = s.spawn(|| client.get_manager_performance(mid));
            let t3 = s.spawn(|| client.get_manager_holding_char(mid));
            (t1.join().unwrap().ok(), t2.join().unwrap().ok(), t3.join().unwrap().ok())
        })
    } else {
        (None, None, None)
    };

    let analysis = FundAnalysis {
        detail,
        periods,
        yearly_returns,
        monthly_returns,
        accumulated_return,
        risk_metrics,
        fee_rules,
        manager_eval,
        manager_char,
        manager_info,
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&analysis)?);
    } else {
        display_analysis(&analysis);
    }

    Ok(())
}

fn display_analysis(a: &FundAnalysis) {
    let d = &a.detail;
    let r = &a.risk_metrics;

    println!();
    println!("{}", format!("━━━ {} {} ━━━", d.code, d.name).bright_cyan().bold());

    println!();
    println!("{}", "基本信息".bright_white().bold());
    let scale: f64 = d.scale.parse().unwrap_or(0.0) / 100_000_000.0;
    println!("  类型: {}  风险: R{}  规模: {:.2}亿", d.fund_type, d.risk_level, scale);
    let mgr_fee: f64 = d.mgr_fee.trim_end_matches('%').parse().unwrap_or(0.0);
    let trust_fee: f64 = d.trust_fee.trim_end_matches('%').parse().unwrap_or(0.0);
    println!("  费率: {:.2}% (管理{:.2}% + 托管{:.2}%)", mgr_fee + trust_fee, mgr_fee, trust_fee);
    if let Some(fee_rules) = &a.fee_rules {
        print_redemption_fee_rules(fee_rules);
    }
    if let Some(info) = &a.manager_info {
        let days: f64 = info.total_days.parse().unwrap_or(0.0);
        let years = days / 365.0;
        let mgr_scale: f64 = info.net_nav.parse().unwrap_or(0.0) / 100_000_000.0;
        println!("  经理: {} (从业{:.1}年, 在管{:.0}亿)", info.manager_name, years, mgr_scale);
    }

    println!();
    println!("{}", "阶段收益".bright_white().bold());
    let periods_to_show = ["Z", "Y", "3Y", "6Y", "1N", "JN"];
    let labels = ["近1周", "近1月", "近3月", "近6月", "近1年", "今年"];

    let mut header = String::from("  ");
    let mut values = String::from("  ");
    let mut ranks = String::from("  排名: ");

    for (i, key) in periods_to_show.iter().enumerate() {
        if let Some(p) = a.periods.iter().find(|pp| {
            (*key == "Z" && pp.title.contains("Week"))
                || (*key == "Y" && pp.title.contains("Month") && !pp.title.contains("3"))
                || (*key == "3Y" && pp.title.contains("3 Month"))
                || (*key == "6Y" && pp.title.contains("6 Month"))
                || (*key == "1N"
                    && pp.title.contains("Year")
                    && !pp.title.contains("2")
                    && !pp.title.contains("3")
                    && !pp.title.contains("5"))
                || (*key == "JN" && pp.title.contains("Date"))
        }) {
            header.push_str(&format!("{:>8}", labels[i]));
            values.push_str(&format!("{:>8}", format!("{:+.2}%", p.return_rate)));
            ranks.push_str(&format!("{:>8}", format!("{}/{}", p.rank, p.total)));
        }
    }
    println!("{}", header.bright_black());
    println!("{}", values);
    println!("{}", ranks.bright_black());

    if !a.yearly_returns.is_empty() {
        println!();
        println!("{}", "年度收益".bright_white().bold());
        for yr in &a.yearly_returns {
            let rank_str =
                if yr.total > 0 { format!("{}/{}", yr.rank, yr.total) } else { "-".to_string() };
            println!("  {}: {}  排名: {}", yr.title, format_pct(yr.return_rate), rank_str);
        }
    }

    println!();
    println!("{}", format!("风险指标 (近{}交易日)", r.data_points).bright_white().bold());
    println!(
        "  年化收益: {}  最大回撤: {}  波动率: {:.2}%  夏普: {:.2}",
        format_pct(r.annualized_return),
        format_pct(r.max_drawdown),
        r.volatility,
        r.sharpe_ratio
    );
    println!(
        "  正收益天: {}  负收益天: {}  日胜率: {:.0}%",
        r.positive_days,
        r.negative_days,
        if r.data_points > 0 {
            r.positive_days as f64 / (r.positive_days + r.negative_days) as f64 * 100.0
        } else {
            0.0
        }
    );
    println!("  月胜率: {:.0}%  超额收益: {}", r.monthly_win_rate, format_pct(r.excess_return));

    if let Some(eval) = &a.manager_eval {
        println!();
        println!("{}", "经理评价".bright_white().bold());
        let dd1: f64 = eval.max_drawdown_1y.parse().unwrap_or(0.0);
        let dd3: f64 = eval.max_drawdown_3y.parse().unwrap_or(0.0);
        let sp1: f64 = eval.sharpe_1y.parse().unwrap_or(0.0);
        let sp3: f64 = eval.sharpe_3y.parse().unwrap_or(0.0);
        let vol1: f64 = eval.volatility_1y.parse().unwrap_or(0.0);
        println!("  近1年回撤: {:.3}%  近3年回撤: {:.3}%", dd1, dd3);
        println!("  近1年夏普: {:.2}  近3年夏普: {:.2}", sp1, sp3);
        println!("  近1年波动率: {:.2}%", vol1);
    }

    if let Some(ch) = &a.manager_char {
        let pos: f64 = ch.stock_position.parse().unwrap_or(0.0);
        let concentration: f64 = ch.top10_concentration.parse().unwrap_or(0.0);
        if pos > 0.0 {
            println!("  股票仓位: {:.1}%  前十集中度: {:.1}%", pos, concentration);
        } else {
            println!("  股票仓位: 0% (纯债策略)");
        }
    }

    let (overall, scores) = scoring::compute_overall_score(
        &a.detail,
        &a.periods,
        &a.yearly_returns,
        &a.risk_metrics,
        &a.manager_eval,
        &a.manager_char,
    );
    println!();
    println!(
        "{} {}",
        "综合评分:".bright_white().bold(),
        format!("{}/100", overall).yellow().bold()
    );
    let score_line: Vec<String> = scores.iter().map(|(name, s)| format!("{}{}", name, s)).collect();
    println!("  {}", score_line.join("  "));

    println!();
    println!("{}", "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".bright_black());
}

fn print_redemption_fee_rules(rules: &FeeRules) {
    if rules.redemption.is_empty() {
        return;
    }

    println!("  卖出费:");
    for rule in &rules.redemption {
        println!("    {} → {}", rule.scope, rule.rate);
    }
}

fn format_pct(value: f64) -> String {
    if value > 0.0 {
        format!("+{:.2}%", value).green().to_string()
    } else if value < 0.0 {
        format!("{:.2}%", value).red().to_string()
    } else {
        format!("{:.2}%", value)
    }
}
