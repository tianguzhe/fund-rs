use anyhow::Result;
use fund_core::api::Client;
use fund_core::models::*;
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
    pub manager_eval: Option<ManagerPerformance>,
    pub manager_char: Option<ManagerHoldingChar>,
    pub manager_info: Option<ManagerInfo>,
}

#[derive(Serialize)]
pub struct RiskMetrics {
    pub annualized_return: f64,
    pub max_drawdown: f64,
    pub volatility: f64,
    pub sharpe_ratio: f64,
    pub positive_days: usize,
    pub negative_days: usize,
    pub monthly_win_rate: f64,
    pub excess_return: f64,
    pub data_points: usize,
}

fn compute_risk_metrics_from_history(points: &[NetValuePoint], monthly_returns: &[PeriodIncrease], acc_return: &[AccumulatedReturn]) -> RiskMetrics {
    if points.len() < 2 {
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

    // API returns newest first, reverse to chronological order
    let mut navs: Vec<f64> = points.iter().map(|p| p.net_value).collect();
    navs.reverse();

    // Max drawdown
    let mut peak = navs[0];
    let mut max_dd = 0.0f64;
    for nav in &navs {
        if *nav > peak {
            peak = *nav;
        }
        let dd = (peak - nav) / peak;
        if dd > max_dd {
            max_dd = dd;
        }
    }

    // Daily returns
    let daily_returns: Vec<f64> = navs.windows(2).map(|w| w[1] / w[0] - 1.0).collect();
    let n = daily_returns.len() as f64;

    // Annualized return
    let total_return = navs.last().unwrap() / navs[0];
    let days = navs.len() as f64;
    let annualized_return = (total_return.powf(250.0 / days) - 1.0) * 100.0;

    // Volatility
    let avg_ret = daily_returns.iter().sum::<f64>() / n;
    let variance = daily_returns.iter().map(|r| (r - avg_ret).powi(2)).sum::<f64>() / n;
    let volatility = variance.sqrt() * (250.0_f64).sqrt() * 100.0;

    // Sharpe ratio (risk-free rate = 2%)
    let sharpe = if volatility > 0.0 {
        (annualized_return - 2.0) / volatility
    } else {
        0.0
    };

    // Positive/negative days
    let positive = daily_returns.iter().filter(|r| **r > 0.0).count();
    let negative = daily_returns.iter().filter(|r| **r < 0.0).count();

    // Monthly win rate from monthly returns
    let monthly_win_rate = if !monthly_returns.is_empty() {
        let wins = monthly_returns.iter().filter(|m| m.return_rate > 0.0).count();
        wins as f64 / monthly_returns.len() as f64 * 100.0
    } else {
        0.0
    };

    // Excess return vs benchmark (from accumulated return)
    let excess_return = if let (Some(first), Some(last)) = (acc_return.first(), acc_return.last()) {
        let fund_ret = last.fund_return - first.fund_return;
        let bench_ret = last.bench_return - first.bench_return;
        fund_ret - bench_ret
    } else {
        0.0
    };

    RiskMetrics {
        annualized_return,
        max_drawdown: max_dd * 100.0,
        volatility,
        sharpe_ratio: sharpe,
        positive_days: positive,
        negative_days: negative,
        monthly_win_rate,
        excess_return,
        data_points: points.len(),
    }
}

fn score_risk(metrics: &RiskMetrics, fund_type: &str) -> u32 {
    let base = if fund_type.contains("债券") || fund_type.contains("债") {
        let dd_score = if metrics.max_drawdown < 0.5 {
            95.0
        } else if metrics.max_drawdown < 1.0 {
            85.0
        } else if metrics.max_drawdown < 2.0 {
            70.0
        } else {
            50.0
        };
        let vol_score = if metrics.volatility < 1.0 {
            95.0
        } else if metrics.volatility < 2.0 {
            80.0
        } else if metrics.volatility < 5.0 {
            65.0
        } else {
            50.0
        };
        dd_score * 0.6 + vol_score * 0.4
    } else {
        let sharpe_score = if metrics.sharpe_ratio > 1.5 {
            90.0
        } else if metrics.sharpe_ratio > 1.0 {
            75.0
        } else if metrics.sharpe_ratio > 0.5 {
            60.0
        } else {
            40.0
        };
        let dd_score = if metrics.max_drawdown < 10.0 {
            85.0
        } else if metrics.max_drawdown < 20.0 {
            70.0
        } else {
            50.0
        };
        sharpe_score * 0.5 + dd_score * 0.5
    };
    base as u32
}

fn score_return(periods: &[PeriodIncrease], metrics: &RiskMetrics) -> u32 {
    let year_ret = periods.iter().find(|p| p.title.contains("1N") || p.title.contains("近1年") || p.title == "Last Year");
    let mut score = if let Some(yr) = year_ret {
        let rank_pct = if yr.total > 0 {
            yr.rank as f64 / yr.total as f64 * 100.0
        } else {
            50.0
        };
        if rank_pct <= 10.0 {
            95
        } else if rank_pct <= 25.0 {
            80
        } else if rank_pct <= 50.0 {
            65
        } else {
            50
        }
    } else {
        50
    };

    // Bonus for excess return
    if metrics.excess_return > 2.0 {
        score = (score + 10).min(95);
    } else if metrics.excess_return > 0.0 {
        score = (score + 5).min(90);
    }

    score
}

fn score_stability(metrics: &RiskMetrics, yearly_returns: &[PeriodIncrease]) -> u32 {
    // Daily win rate
    let daily_win = if metrics.data_points > 0 {
        metrics.positive_days as f64 / (metrics.positive_days + metrics.negative_days) as f64 * 100.0
    } else {
        50.0
    };

    // Monthly win rate
    let monthly_win = metrics.monthly_win_rate;

    // Yearly consistency
    let yearly_score = if yearly_returns.len() >= 2 {
        let all_positive = yearly_returns.iter().all(|y| y.return_rate > 0.0);
        if all_positive { 90 } else { 65 }
    } else {
        70
    };

    let win_score = if daily_win >= 60.0 {
        90
    } else if daily_win >= 55.0 {
        80
    } else if daily_win >= 50.0 {
        70
    } else {
        55
    };

    let monthly_score = if monthly_win >= 80.0 {
        90
    } else if monthly_win >= 60.0 {
        80
    } else if monthly_win >= 50.0 {
        70
    } else {
        55
    };

    (win_score as f64 * 0.3 + monthly_score as f64 * 0.4 + yearly_score as f64 * 0.3) as u32
}

fn score_fees(detail: &FundDetail) -> u32 {
    let mgr: f64 = detail.mgr_fee.trim_end_matches('%').parse().unwrap_or(0.5);
    let trust: f64 = detail.trust_fee.trim_end_matches('%').parse().unwrap_or(0.1);
    let total = mgr + trust;
    if total <= 0.20 {
        95
    } else if total <= 0.35 {
        80
    } else if total <= 0.50 {
        70
    } else if total <= 1.00 {
        55
    } else {
        40
    }
}

fn score_scale(detail: &FundDetail) -> u32 {
    let scale: f64 = detail.scale.parse().unwrap_or(0.0) / 100_000_000.0;
    if detail.fund_type.contains("货币") {
        if scale >= 100.0 {
            90
        } else if scale >= 10.0 {
            75
        } else {
            50
        }
    } else if detail.fund_type.contains("指数") || detail.fund_type.contains("ETF") {
        if scale >= 10.0 {
            85
        } else if scale >= 1.0 {
            70
        } else {
            50
        }
    } else {
        if scale >= 5.0 && scale <= 100.0 {
            90
        } else if scale >= 2.0 && scale <= 200.0 {
            75
        } else {
            55
        }
    }
}

fn score_manager(eval: &Option<ManagerPerformance>) -> u32 {
    if let Some(e) = eval {
        let sharpe: f64 = e.sharpe_1y.parse().unwrap_or(0.0);
        let dd: f64 = e.max_drawdown_1y.parse().unwrap_or(100.0);
        let mut score = 70;
        if sharpe > 1.0 {
            score += 10;
        }
        if dd < 0.05 {
            score += 10;
        }
        score.min(95)
    } else {
        60
    }
}

fn score_holding_style(char_data: &Option<ManagerHoldingChar>) -> u32 {
    if let Some(ch) = char_data {
        let stock_pos: f64 = ch.stock_position.parse().unwrap_or(0.0);
        let concentration: f64 = ch.top10_concentration.parse().unwrap_or(50.0);

        // For bond funds, lower stock position = better
        let pos_score = if stock_pos <= 5.0 {
            90.0
        } else if stock_pos <= 15.0 {
            75.0
        } else {
            55.0
        };

        let conc_score = if concentration <= 30.0 {
            85.0
        } else if concentration <= 50.0 {
            70.0
        } else {
            55.0
        };

        (pos_score * 0.6 + conc_score * 0.4) as u32
    } else {
        70
    }
}

fn compute_overall_score(analysis: &FundAnalysis) -> (u32, Vec<(String, u32)>) {
    let fund_type = &analysis.detail.fund_type;
    let is_bond = fund_type.contains("债券") || fund_type.contains("债");

    let style_score = score_holding_style(&analysis.manager_char);

    let weights: Vec<(&str, u32, u32)> = if is_bond {
        vec![
            ("收益", score_return(&analysis.periods, &analysis.risk_metrics), 15),
            ("风险", score_risk(&analysis.risk_metrics, fund_type), 30),
            ("稳定", score_stability(&analysis.risk_metrics, &analysis.yearly_returns), 20),
            ("费用", score_fees(&analysis.detail), 15),
            ("规模", score_scale(&analysis.detail), 10),
            ("经理", score_manager(&analysis.manager_eval), 5),
            ("风格", style_score, 5),
        ]
    } else {
        vec![
            ("收益", score_return(&analysis.periods, &analysis.risk_metrics), 25),
            ("风险", score_risk(&analysis.risk_metrics, fund_type), 30),
            ("稳定", score_stability(&analysis.risk_metrics, &analysis.yearly_returns), 15),
            ("费用", score_fees(&analysis.detail), 10),
            ("规模", score_scale(&analysis.detail), 10),
            ("经理", score_manager(&analysis.manager_eval), 5),
            ("风格", style_score, 5),
        ]
    };

    let total: u32 = weights.iter().map(|(_, s, w)| s * w).sum();
    let overall = total / 100;
    let details: Vec<(String, u32)> = weights.into_iter().map(|(n, s, _)| (n.to_string(), s)).collect();
    (overall, details)
}

pub fn run(client: &Client, code: &str, json: bool) -> Result<()> {
    eprintln!("查询基金 {} ...", code);

    // 1. 基金详情
    let detail = client.get_fund_estimate(code)?;

    // 2. 阶段涨幅
    let periods = client.get_period_increase(code)?;

    // 3. 年度收益
    let yearly_returns = client.get_yearly_returns(code).unwrap_or_default();

    // 4. 月度收益
    let monthly_returns = client.get_monthly_returns(code).unwrap_or_default();

    // 5. 累计收益 vs 基准
    let accumulated_return = client.get_accumulated_return(code, "ln", "000300").unwrap_or_default();

    // 6. 净值走势 (用于计算风险指标)
    // fundVPageDiagram 对部分基金返回空数据，降级用 fundMNHisNetList
    let history = client.get_net_value_history(code, 250).unwrap_or_default();
    let risk_metrics = compute_risk_metrics_from_history(&history, &monthly_returns, &accumulated_return);

    // 7. 基金经理
    let managers = client.get_fund_managers(code).unwrap_or_default();
    let manager_id = managers.first().map(|m| m.manager_id.clone());

    let (manager_info, manager_eval, manager_char) = if let Some(mid) = &manager_id {
        eprintln!("查询经理 {} ...", mid);
        let info = client.get_manager_info(mid).ok();
        let eval = client.get_manager_performance(mid).ok();
        let char = client.get_manager_holding_char(mid).ok();
        (info, eval, char)
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
        manager_eval,
        manager_char,
        manager_info,
    };

    if json {
        let output = serde_json::to_string_pretty(&analysis)?;
        println!("{}", output);
    } else {
        display_analysis(&analysis);
    }

    Ok(())
}

fn display_analysis(a: &FundAnalysis) {
    let d = &a.detail;
    let r = &a.risk_metrics;

    // Header
    println!();
    println!(
        "{}",
        format!("━━━ {} {} ━━━", d.code, d.name).bright_cyan().bold()
    );

    // Basic info
    println!();
    println!("{}", "基本信息".bright_white().bold());
    let scale: f64 = d.scale.parse().unwrap_or(0.0) / 100_000_000.0;
    println!(
        "  类型: {}  风险: R{}  规模: {:.2}亿",
        d.fund_type, d.risk_level, scale
    );
    let mgr_fee: f64 = d.mgr_fee.trim_end_matches('%').parse().unwrap_or(0.0);
    let trust_fee: f64 = d.trust_fee.trim_end_matches('%').parse().unwrap_or(0.0);
    println!(
        "  费率: {:.2}% (管理{:.2}% + 托管{:.2}%)",
        mgr_fee + trust_fee,
        mgr_fee,
        trust_fee
    );
    if let Some(info) = &a.manager_info {
        let days: f64 = info.total_days.parse().unwrap_or(0.0);
        let years = days / 365.0;
        let mgr_scale: f64 = info.net_nav.parse().unwrap_or(0.0) / 100_000_000.0;
        println!(
            "  经理: {} (从业{:.1}年, 在管{:.0}亿)",
            info.manager_name, years, mgr_scale
        );
    }

    // Period returns
    println!();
    println!("{}", "阶段收益".bright_white().bold());
    let periods_to_show = ["Z", "Y", "3Y", "6Y", "1N", "JN"];
    let labels = ["近1周", "近1月", "近3月", "近6月", "近1年", "今年"];

    let mut header = String::from("  ");
    let mut values = String::from("  ");
    let mut ranks = String::from("  排名: ");

    for (i, key) in periods_to_show.iter().enumerate() {
        if let Some(p) = a.periods.iter().find(|pp| {
            (key == &"Z" && pp.title.contains("Week"))
                || (key == &"Y" && pp.title.contains("Month") && !pp.title.contains("3"))
                || (key == &"3Y" && pp.title.contains("3 Month"))
                || (key == &"6Y" && pp.title.contains("6 Month"))
                || (key == &"1N" && pp.title.contains("Year") && !pp.title.contains("2") && !pp.title.contains("3") && !pp.title.contains("5"))
                || (key == &"JN" && pp.title.contains("Date"))
        }) {
            header.push_str(&format!("{:>8}", labels[i]));
            let val_str = format!("{:+.2}%", p.return_rate);
            values.push_str(&format!("{:>8}", val_str));
            ranks.push_str(&format!("{:>8}", format!("{}/{}", p.rank, p.total)));
        }
    }
    println!("{}", header.bright_black());
    println!("{}", values);
    println!("{}", ranks.bright_black());

    // Yearly returns
    if !a.yearly_returns.is_empty() {
        println!();
        println!("{}", "年度收益".bright_white().bold());
        for yr in &a.yearly_returns {
            let rank_str = if yr.total > 0 {
                format!("{}/{}", yr.rank, yr.total)
            } else {
                "-".to_string()
            };
            println!(
                "  {}: {}  排名: {}",
                yr.title,
                format_pct(yr.return_rate),
                rank_str
            );
        }
    }

    // Risk metrics
    println!();
    println!(
        "{}",
        format!("风险指标 (近{}交易日)", r.data_points).bright_white().bold()
    );
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
    println!(
        "  月胜率: {:.0}%  超额收益: {}",
        r.monthly_win_rate,
        format_pct(r.excess_return)
    );

    // Manager evaluation
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

    // Overall score
    let (overall, scores) = compute_overall_score(a);
    println!();
    println!(
        "{} {}",
        "综合评分:".bright_white().bold(),
        format!("{}/100", overall).yellow().bold()
    );
    let score_line: Vec<String> = scores
        .iter()
        .map(|(name, s)| format!("{}{}", name, s))
        .collect();
    println!("  {}", score_line.join("  "));

    println!();
    println!("{}", "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".bright_black());
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
