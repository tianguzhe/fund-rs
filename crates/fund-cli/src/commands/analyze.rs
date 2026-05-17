use anyhow::Result;
use fund_core::api::{aggregate_monthly_returns, Client};
use fund_core::f10::{
    AssetAllocationPoint, DividendRecord, FeeRules, HolderStructurePoint, HoldingConstraints,
    ScaleChangePoint, TopBondsReport, TopStocksReport,
};
use fund_core::models::*;
use fund_core::scoring::{self, BenchmarkMetrics, DistributionStats, RiskMetrics, RollingReturns};
use owo_colors::OwoColorize;
use serde::Serialize;

#[derive(Serialize)]
pub struct ScoreItem {
    pub name: String,
    pub score: u32,
    pub weight: u32,
}

#[derive(Serialize)]
pub struct ScoreBreakdown {
    pub overall: u32,
    pub items: Vec<ScoreItem>,
}

/// Per-section data freshness for downstream UIs to surface "as of" labels.
/// Each field reports the latest date observed in that section's payload, or
/// None when the section is empty / failed to fetch.
#[derive(Serialize)]
pub struct AnalysisMeta {
    pub generated_at: String,
    pub as_of: AsOf,
}

#[derive(Serialize)]
pub struct AsOf {
    pub nav_history: Option<String>,
    pub nav_trend: Option<String>,
    pub accumulated_return: Option<String>,
    pub monthly_series: Option<String>,
    pub top_stocks: Option<String>,
    pub top_bonds: Option<String>,
    pub asset_allocation: Option<String>,
    pub scale_changes: Option<String>,
    pub holder_structure: Option<String>,
}

#[derive(Serialize)]
pub struct FundAnalysis {
    pub detail: FundDetail,
    pub periods: Vec<PeriodIncrease>,
    pub yearly_returns: Vec<PeriodIncrease>,
    pub monthly_returns: Vec<PeriodIncrease>,
    /// True month-by-month return series locally aggregated from daily NAV history.
    /// Distinct from `monthly_returns`, which echoes the rolling-period enum.
    pub monthly_series: Vec<MonthlyReturnPoint>,
    /// Daily unit NAV + accumulated NAV history (most recent ≤ N trading days).
    pub nav_history: Vec<NetValuePoint>,
    pub accumulated_return: Vec<AccumulatedReturn>,
    pub risk_metrics: RiskMetrics,
    /// Benchmark-relative metrics (alpha/beta/IR/TE) derived from accumulated_return.
    pub benchmark_metrics: BenchmarkMetrics,
    /// Tail-risk + shape descriptors of daily return distribution.
    pub distribution: DistributionStats,
    /// Rolling 1Y / 3Y return distribution stats.
    pub rolling_returns: RollingReturns,
    pub fee_rules: Option<FeeRules>,
    /// Bond holdings (zqcc). Only populated for bond-type funds.
    pub top_bonds: Option<TopBondsReport>,
    /// Top-10 stock holdings (jjcc). Skipped for funds with no equity exposure.
    pub top_stocks: Option<TopStocksReport>,
    /// Asset allocation history from F10 zcpz page (stock / bond / cash %).
    pub asset_allocation: Vec<AssetAllocationPoint>,
    /// Historical scale + flows from F10 gmbd.
    pub scale_changes: Vec<ScaleChangePoint>,
    /// Holder structure history (institutional / retail / internal) from F10 cyrjg.
    pub holder_structure: Vec<HolderStructurePoint>,
    /// Dividend history from F10 fhsp.
    pub dividends: Vec<DividendRecord>,
    /// Purchase/redemption status + minimum holding period from F10 jbgk.
    pub holding_constraints: Option<HoldingConstraints>,
    pub manager_eval: Option<ManagerPerformance>,
    pub manager_char: Option<ManagerHoldingChar>,
    pub manager_info: Option<ManagerInfo>,
    /// Funds the current manager has managed (current + historical), structured.
    pub manager_history: Vec<ManagerHistoryFund>,
    pub scores: ScoreBreakdown,
    pub meta: AnalysisMeta,
}

pub fn run(client: &Client, code: &str, json: bool, output: Option<&std::path::Path>) -> Result<()> {
    eprintln!("查询基金 {} ...", code);

    // Batch 1: independent requests in parallel — fund-core API + F10 + local NAV aggregation.
    let (
        detail_r,
        periods_r,
        yearly_r,
        monthly_r,
        nav_full_r,
        nav_trend_r,
        managers_r,
        scale_changes_r,
        holder_structure_r,
        dividends_r,
        asset_allocation_r,
    ) = std::thread::scope(|s| {
        let t1 = s.spawn(|| client.get_fund_estimate(code));
        let t2 = s.spawn(|| client.get_period_increase(code));
        let t3 = s.spawn(|| client.get_yearly_returns(code));
        let t4 = s.spawn(|| client.get_monthly_returns(code));
        // ~36 months of daily NAV: feeds both monthly_series (aggregated locally)
        // and nav_history (the most recent slice exposed verbatim).
        let t5 = s.spawn(|| client.get_net_value_history(code, 820));
        // 3-year window covers at least one full market cycle for risk metrics.
        let t6 = s.spawn(|| client.get_nav_trend(code, "3n", 500));
        let t7 = s.spawn(|| client.get_fund_managers(code));
        let t8 = s.spawn(|| fund_core::f10::get_scale_changes(code));
        let t9 = s.spawn(|| fund_core::f10::get_holder_structure(code));
        let t10 = s.spawn(|| fund_core::f10::get_dividends(code));
        let t11 = s.spawn(|| fund_core::f10::get_asset_allocation(code));
        (
            t1.join().unwrap(),
            t2.join().unwrap(),
            t3.join().unwrap(),
            t4.join().unwrap(),
            t5.join().unwrap(),
            t6.join().unwrap(),
            t7.join().unwrap(),
            t8.join().unwrap(),
            t9.join().unwrap(),
            t10.join().unwrap(),
            t11.join().unwrap(),
        )
    });

    let detail = detail_r?;
    let periods = periods_r?;
    let yearly_returns = yearly_r.unwrap_or_default();
    let monthly_returns = monthly_r.unwrap_or_default();
    let nav_full = nav_full_r.unwrap_or_default();
    // Derive monthly series from the same NAV pull instead of re-fetching.
    let monthly_series = aggregate_monthly_returns(&nav_full, 36);
    // Expose only the most recent ~60 trading days to keep JSON payload bounded.
    let nav_history = {
        let mut sorted: Vec<NetValuePoint> = nav_full.to_vec();
        sorted.sort_by(|a, b| b.date.cmp(&a.date));
        sorted.into_iter().take(60).collect::<Vec<_>>()
    };
    let nav_trend = nav_trend_r.unwrap_or_default();
    let managers = managers_r.unwrap_or_default();
    let scale_changes = scale_changes_r.unwrap_or_default();
    let holder_structure = holder_structure_r.unwrap_or_default();
    let dividends = dividends_r.unwrap_or_default();
    let asset_allocation = asset_allocation_r.unwrap_or_default();
    // Pure parser over fund name — cheap, deterministic, no network.
    let holding_constraints =
        Some(fund_core::f10::detect_holding_constraints(&detail.name, &detail.full_name));
    let fee_rules = fund_core::f10::get_fee_rules(code).ok();

    // Bond holdings only make sense for bond-type funds; skip the F10 round-trip otherwise.
    let (cy, cm) = current_year_month();
    let (qy, qm) = fund_core::f10::latest_quarter_end(cy, cm);
    let top_bonds = if detail.fund_type.contains("债") {
        fund_core::f10::get_top_bonds(code, qy, qm).ok()
    } else {
        None
    };

    // Top-10 stock holdings — relevant whenever there is any equity exposure.
    // Cheapest signal: 货币 / 纯债 / 短债 / 中短债 declare zero equity by design.
    // Two-tier check so 二级债基 / 偏债混合 / 灵活配置 / 股票 / 指数 all get stocks.
    let has_equity = !(detail.fund_type.contains("货币")
        || detail.fund_type.contains("纯债")
        || detail.fund_type.contains("短债"));
    let top_stocks =
        if has_equity { fund_core::f10::get_top_stocks(code, qy, qm).ok() } else { None };

    // Use INDEXCODE from fund detail for index/ETF funds; fallback to type-based selection.
    let benchmark = scoring::select_benchmark(&detail.fund_type, &detail.index_code);
    let accumulated_return =
        client.get_accumulated_return(code, "ln", &benchmark).unwrap_or_default();

    let risk_metrics =
        scoring::compute_risk_metrics(&nav_trend, &monthly_returns, &accumulated_return);
    let benchmark_metrics = scoring::compute_benchmark_metrics(&accumulated_return);
    let distribution = scoring::compute_distribution_stats(&nav_trend);
    let rolling_returns = scoring::compute_rolling_returns(&nav_full);

    // Batch 2: manager details in parallel (depends on manager_id from batch 1)
    let manager_id = managers.into_iter().next().map(|m| m.manager_id);
    let (manager_info, manager_eval, manager_char, manager_history) = if let Some(mid) = &manager_id
    {
        eprintln!("查询经理 {} ...", mid);
        std::thread::scope(|s| {
            let t1 = s.spawn(|| client.get_manager_info(mid));
            let t2 = s.spawn(|| client.get_manager_performance(mid));
            let t3 = s.spawn(|| client.get_manager_holding_char(mid));
            let t4 = s.spawn(|| client.get_manager_history_funds(mid));
            (
                t1.join().unwrap().ok(),
                t2.join().unwrap().ok(),
                t3.join().unwrap().ok(),
                t4.join().unwrap().unwrap_or_default(),
            )
        })
    } else {
        (None, None, None, Vec::new())
    };

    let (overall, score_details) = scoring::compute_overall_score(
        &detail,
        &periods,
        &yearly_returns,
        &risk_metrics,
        &manager_eval,
        &manager_char,
    );
    let is_bond = detail.fund_type.contains("债");
    let weights: &[(&str, u32)] = if is_bond {
        &[
            ("收益", 15),
            ("风险", 30),
            ("稳定", 20),
            ("费用", 15),
            ("规模", 10),
            ("经理", 5),
            ("风格", 5),
        ]
    } else {
        &[
            ("收益", 25),
            ("风险", 30),
            ("稳定", 15),
            ("费用", 10),
            ("规模", 10),
            ("经理", 5),
            ("风格", 5),
        ]
    };
    let scores = ScoreBreakdown {
        overall,
        items: score_details
            .into_iter()
            .zip(weights.iter())
            .map(|((name, score), (_, weight))| ScoreItem { name, score, weight: *weight })
            .collect(),
    };

    // Compose per-section as_of: latest date observed in each section's payload.
    let meta = AnalysisMeta {
        generated_at: today_ymd(),
        as_of: AsOf {
            nav_history: nav_history.first().map(|p| p.date.clone()),
            nav_trend: nav_trend.iter().map(|p| p.date.clone()).max(),
            accumulated_return: accumulated_return.last().map(|p| p.date.clone()),
            monthly_series: monthly_series.last().map(|p| p.month.clone()),
            top_stocks: top_stocks.as_ref().map(|t| t.end_date.clone()),
            top_bonds: top_bonds.as_ref().map(|t| t.end_date.clone()),
            asset_allocation: asset_allocation.first().map(|p| p.date.clone()),
            scale_changes: scale_changes.first().map(|p| p.date.clone()),
            holder_structure: holder_structure.first().map(|p| p.announce_date.clone()),
        },
    };

    let analysis = FundAnalysis {
        detail,
        periods,
        yearly_returns,
        monthly_returns,
        monthly_series,
        nav_history,
        accumulated_return,
        risk_metrics,
        benchmark_metrics,
        distribution,
        rolling_returns,
        fee_rules,
        top_bonds,
        top_stocks,
        asset_allocation,
        scale_changes,
        holder_structure,
        dividends,
        holding_constraints,
        manager_eval,
        manager_char,
        manager_info,
        manager_history,
        scores,
        meta,
    };

    if json {
        let payload = serde_json::to_string_pretty(&analysis)?;
        if let Some(path) = output {
            if let Some(parent) = path.parent() {
                if !parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(parent)?;
                }
            }
            std::fs::write(path, &payload)?;
            eprintln!("已写入 {}", path.display());
        } else {
            println!("{}", payload);
        }
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

// Local-time year/month without pulling chrono. Matches the holdings.rs helper —
// extracting to a shared util belongs to a later cleanup batch.
fn current_year_month() -> (u32, u32) {
    let (y, m, _d) = current_ymd();
    (y, m)
}

fn today_ymd() -> String {
    let (y, m, d) = current_ymd();
    format!("{:04}-{:02}-{:02}", y, m, d)
}

fn current_ymd() -> (u32, u32, u32) {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = (secs / 86400) as i32;
    let mut y = 1970i32;
    let mut d = days;
    loop {
        let leap = (y % 4 == 0 && y % 100 != 0) || (y % 400 == 0);
        let yd = if leap { 366 } else { 365 };
        if d < yd {
            break;
        }
        d -= yd;
        y += 1;
    }
    let months = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let leap = (y % 4 == 0 && y % 100 != 0) || (y % 400 == 0);
    let mut m = 0usize;
    let mut dd = d;
    while m < 12 {
        let md = if m == 1 && leap { 29 } else { months[m] };
        if dd < md {
            break;
        }
        dd -= md;
        m += 1;
    }
    (y as u32, (m + 1) as u32, (dd + 1) as u32)
}
