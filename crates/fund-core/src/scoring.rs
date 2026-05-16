use crate::models::{
    AccumulatedReturn, FundDetail, ManagerHoldingChar, ManagerPerformance, NavTrendPoint,
    PeriodIncrease,
};
use serde::Serialize;

#[derive(Debug, Serialize)]
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
    /// Calmar ratio = annualized_return / max_drawdown.
    /// Caps at 99 to avoid Inf when max_drawdown is essentially zero.
    pub calmar_ratio: f64,
    /// Sortino ratio = (annualized_return - rf) / downside_volatility.
    /// Downside volatility uses only negative daily returns (target = 0).
    pub sortino_ratio: f64,
    /// Drawdown at the latest data point (% of peak). 0 if currently at a new high.
    pub current_drawdown: f64,
    /// Calendar days from the trough of the max-drawdown episode back to a new
    /// equal-or-higher peak. `None` means the fund has not yet recovered.
    pub max_drawdown_recovery_days: Option<i64>,
}

pub fn compute_risk_metrics(
    points: &[NavTrendPoint],
    monthly_returns: &[PeriodIncrease],
    acc_return: &[AccumulatedReturn],
) -> RiskMetrics {
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
            calmar_ratio: 0.0,
            sortino_ratio: 0.0,
            current_drawdown: 0.0,
            max_drawdown_recovery_days: None,
        };
    }

    // Sort by date (YYYY-MM-DD) to guarantee chronological order regardless of API response ordering.
    let mut sorted: Vec<&NavTrendPoint> = points.iter().collect();
    sorted.sort_by(|a, b| a.date.cmp(&b.date));
    let navs: Vec<f64> = sorted.iter().map(|p| p.nav).collect();

    // Track both the magnitude of max drawdown AND when it bottomed, so we can
    // measure recovery time afterwards.
    let mut peak = navs[0];
    let mut max_dd = 0.0f64;
    let mut max_dd_peak_idx = 0usize;
    let mut max_dd_trough_idx = 0usize;
    let mut running_peak_idx = 0usize;
    for (i, nav) in navs.iter().enumerate() {
        if *nav > peak {
            peak = *nav;
            running_peak_idx = i;
        }
        let dd = (peak - nav) / peak;
        if dd > max_dd {
            max_dd = dd;
            max_dd_peak_idx = running_peak_idx;
            max_dd_trough_idx = i;
        }
    }

    let daily_returns: Vec<f64> = navs.windows(2).map(|w| w[1] / w[0] - 1.0).collect();
    let n = daily_returns.len() as f64;
    let total_return = navs.last().unwrap() / navs[0];
    let days = navs.len() as f64;
    let annualized_return = (total_return.powf(250.0 / days) - 1.0) * 100.0;

    let avg_ret = daily_returns.iter().sum::<f64>() / n;
    let variance = daily_returns.iter().map(|r| (r - avg_ret).powi(2)).sum::<f64>() / n;
    // Annualized volatility (250 trading days)
    let volatility = variance.sqrt() * 250.0_f64.sqrt() * 100.0;

    // Sharpe ratio with risk-free rate = 2%
    let sharpe = if volatility > 0.0 { (annualized_return - 2.0) / volatility } else { 0.0 };

    // Downside volatility for Sortino: only negative daily returns, target = 0.
    let downside_sq_sum: f64 = daily_returns.iter().filter(|r| **r < 0.0).map(|r| r.powi(2)).sum();
    let downside_vol = (downside_sq_sum / n).sqrt() * 250.0_f64.sqrt() * 100.0;
    let sortino = if downside_vol > 0.0 { (annualized_return - 2.0) / downside_vol } else { 0.0 };

    // Calmar = annualized return / max drawdown. Cap at 99 when MDD ≈ 0 to keep
    // JSON output finite — funds with truly zero historical drawdown are rare
    // and this ceiling is meaningful for downstream UI ranking.
    let calmar =
        if max_dd * 100.0 > 0.01 { (annualized_return / (max_dd * 100.0)).min(99.0) } else { 0.0 };

    // Current drawdown: distance from the running peak at the most recent point.
    let current_peak = navs[..navs.len()].iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let last_nav = *navs.last().unwrap();
    let current_drawdown = if current_peak > 0.0 {
        ((current_peak - last_nav) / current_peak).max(0.0) * 100.0
    } else {
        0.0
    };

    // Recovery time: how long from the max-drawdown trough did it take for NAV
    // to climb back to the pre-drawdown peak. None if still under water.
    let max_dd_recovery_days = recovery_days(&sorted, max_dd_peak_idx, max_dd_trough_idx);

    let positive = daily_returns.iter().filter(|r| **r > 0.0).count();
    let negative = daily_returns.iter().filter(|r| **r < 0.0).count();

    let monthly_win_rate = if !monthly_returns.is_empty() {
        let wins = monthly_returns.iter().filter(|m| m.return_rate > 0.0).count();
        wins as f64 / monthly_returns.len() as f64 * 100.0
    } else {
        0.0
    };

    let excess_return = if let (Some(first), Some(last)) = (acc_return.first(), acc_return.last()) {
        (last.fund_return - first.fund_return) - (last.bench_return - first.bench_return)
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
        calmar_ratio: calmar,
        sortino_ratio: sortino,
        current_drawdown,
        max_drawdown_recovery_days: max_dd_recovery_days,
    }
}

/// Calendar days between the pre-drawdown peak and the first subsequent date
/// where NAV recovers to that peak value. Returns None if recovery never
/// happened within the sample. Uses simple lexicographic date diff so a
/// proper date parser is unnecessary — input dates are already YYYY-MM-DD.
fn recovery_days(sorted: &[&NavTrendPoint], peak_idx: usize, trough_idx: usize) -> Option<i64> {
    if peak_idx >= sorted.len() || trough_idx >= sorted.len() {
        return None;
    }
    let peak_nav = sorted[peak_idx].nav;
    for p in sorted.iter().skip(trough_idx + 1) {
        if p.nav >= peak_nav {
            return calendar_day_diff(&sorted[trough_idx].date, &p.date);
        }
    }
    None
}

/// Calendar day diff between two YYYY-MM-DD strings. Returns None on parse error.
fn calendar_day_diff(start: &str, end: &str) -> Option<i64> {
    let s = parse_ymd(start)?;
    let e = parse_ymd(end)?;
    Some(days_from_civil(e) - days_from_civil(s))
}

fn parse_ymd(s: &str) -> Option<(i32, u32, u32)> {
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != 3 {
        return None;
    }
    Some((parts[0].parse().ok()?, parts[1].parse().ok()?, parts[2].parse().ok()?))
}

/// Howard Hinnant's days_from_civil algorithm — proleptic Gregorian, no Y/M overflow concerns.
fn days_from_civil((y, m, d): (i32, u32, u32)) -> i64 {
    let y = if m <= 2 { y - 1 } else { y } as i64;
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = (y - era * 400) as u64;
    let m = m as u64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d as u64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe as i64 - 719468
}

/// Benchmark-relative metrics derived from the `accumulated_return` series.
/// All four fields are annualized and report None when there is not enough
/// overlap with non-trivial benchmark variance to make the math meaningful.
#[derive(Debug, Serialize)]
pub struct BenchmarkMetrics {
    /// CAPM beta — fund daily return regressed on benchmark daily return.
    pub beta: Option<f64>,
    /// Jensen's alpha, annualized (%/year). Risk-free rate = 2%.
    pub alpha: Option<f64>,
    /// Tracking error (annualized stdev of fund - benchmark daily return), %.
    pub tracking_error: Option<f64>,
    /// Information ratio (annualized): mean(active) / stdev(active) × √250.
    pub information_ratio: Option<f64>,
    /// Number of overlapping daily return points used.
    pub data_points: usize,
}

impl BenchmarkMetrics {
    pub fn empty() -> Self {
        Self {
            beta: None,
            alpha: None,
            tracking_error: None,
            information_ratio: None,
            data_points: 0,
        }
    }
}

/// Compute benchmark-relative metrics from cumulative return series.
///
/// `acc_return` rows carry `fund_return` / `bench_return` as cumulative %
/// since the series start. We diff them into daily returns:
///   r_t = (1 + cum_t/100) / (1 + cum_{t-1}/100) - 1
/// then run OLS on (r_bench, r_fund) for beta/alpha, and stdev/mean on
/// active return for TE / IR. Rows where bench_return is unavailable (all
/// zeros — Eastmoney returns 0 when INDEXCODE doesn't apply) collapse to
/// `BenchmarkMetrics::empty()` so downstream callers don't divide by zero.
pub fn compute_benchmark_metrics(acc_return: &[AccumulatedReturn]) -> BenchmarkMetrics {
    if acc_return.len() < 30 {
        return BenchmarkMetrics::empty();
    }
    let mut sorted: Vec<&AccumulatedReturn> = acc_return.iter().collect();
    sorted.sort_by(|a, b| a.date.cmp(&b.date));

    let to_daily = |key: fn(&AccumulatedReturn) -> f64| -> Vec<f64> {
        sorted
            .windows(2)
            .map(|w| {
                let a = 1.0 + key(w[0]) / 100.0;
                let b = 1.0 + key(w[1]) / 100.0;
                if a == 0.0 {
                    0.0
                } else {
                    b / a - 1.0
                }
            })
            .collect()
    };
    let r_fund = to_daily(|p| p.fund_return);
    let r_bench = to_daily(|p| p.bench_return);

    // Reject if benchmark is essentially flat — common when bench data is missing.
    let bench_var: f64 = {
        let mean = r_bench.iter().sum::<f64>() / r_bench.len() as f64;
        r_bench.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / r_bench.len() as f64
    };
    if bench_var < 1e-12 {
        return BenchmarkMetrics::empty();
    }

    let n = r_fund.len() as f64;
    let mean_f = r_fund.iter().sum::<f64>() / n;
    let mean_b = r_bench.iter().sum::<f64>() / n;
    let cov: f64 =
        r_fund.iter().zip(r_bench.iter()).map(|(f, b)| (f - mean_f) * (b - mean_b)).sum::<f64>()
            / n;
    let beta = cov / bench_var;
    // Annualized Jensen's alpha. Daily rf ≈ 2%/250 = 0.00008.
    let daily_rf = 0.02 / 250.0;
    let alpha_daily = mean_f - daily_rf - beta * (mean_b - daily_rf);
    let alpha_annual = alpha_daily * 250.0 * 100.0;

    let active: Vec<f64> = r_fund.iter().zip(r_bench.iter()).map(|(f, b)| f - b).collect();
    let mean_a = active.iter().sum::<f64>() / n;
    let te_daily = (active.iter().map(|x| (x - mean_a).powi(2)).sum::<f64>() / n).sqrt();
    let tracking_error = te_daily * 250.0_f64.sqrt() * 100.0;
    let ir = if te_daily > 0.0 { mean_a / te_daily * 250.0_f64.sqrt() } else { 0.0 };

    BenchmarkMetrics {
        beta: Some(beta),
        alpha: Some(alpha_annual),
        tracking_error: Some(tracking_error),
        information_ratio: Some(ir),
        data_points: r_fund.len(),
    }
}

/// 选择超额收益对比基准指数代码。
/// 优先使用基金详情中的跟踪指数代码（INDEXCODE），适用于指数/ETF 基金；
/// 其次按类型名判断：债券基金用中债总指数（H11001），其余用沪深300（000300）。
pub fn select_benchmark(fund_type: &str, index_code: &str) -> String {
    if !index_code.is_empty() {
        return index_code.to_string();
    }
    if fund_type.contains("债") {
        "H11001".to_string()
    } else {
        "000300".to_string()
    }
}

pub fn score_return(periods: &[PeriodIncrease], metrics: &RiskMetrics) -> u32 {
    let year_ret = periods
        .iter()
        .find(|p| p.title.contains("1N") || p.title.contains("近1年") || p.title == "Last Year");
    let mut score = if let Some(yr) = year_ret {
        let rank_pct = if yr.total > 0 { yr.rank as f64 / yr.total as f64 * 100.0 } else { 50.0 };
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
    if metrics.excess_return > 2.0 {
        score = (score + 10).min(95);
    } else if metrics.excess_return > 0.0 {
        score = (score + 5).min(90);
    }
    score
}

pub fn score_risk(metrics: &RiskMetrics, fund_type: &str) -> u32 {
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

pub fn score_stability(metrics: &RiskMetrics, yearly_returns: &[PeriodIncrease]) -> u32 {
    let daily_win = if metrics.data_points > 0 {
        metrics.positive_days as f64 / (metrics.positive_days + metrics.negative_days) as f64
            * 100.0
    } else {
        50.0
    };
    let monthly_win = metrics.monthly_win_rate;
    let yearly_score = if yearly_returns.len() >= 2 {
        if yearly_returns.iter().all(|y| y.return_rate > 0.0) {
            90
        } else {
            65
        }
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

pub fn score_fees(detail: &FundDetail) -> u32 {
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

pub fn score_scale(detail: &FundDetail) -> u32 {
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
    } else if (5.0..=100.0).contains(&scale) {
        90
    } else if (2.0..=200.0).contains(&scale) {
        75
    } else {
        55
    }
}

pub fn score_manager(eval: &Option<ManagerPerformance>) -> u32 {
    if let Some(e) = eval {
        let sharpe: f64 = e.sharpe_1y.parse().unwrap_or(0.0);
        let dd: f64 = e.max_drawdown_1y.parse().unwrap_or(100.0);
        let mut score = 70u32;
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

pub fn score_holding_style(char_data: &Option<ManagerHoldingChar>) -> u32 {
    if let Some(ch) = char_data {
        let stock_pos: f64 = ch.stock_position.parse().unwrap_or(0.0);
        let concentration: f64 = ch.top10_concentration.parse().unwrap_or(50.0);
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

pub fn compute_overall_score(
    detail: &FundDetail,
    periods: &[PeriodIncrease],
    yearly_returns: &[PeriodIncrease],
    risk_metrics: &RiskMetrics,
    manager_eval: &Option<ManagerPerformance>,
    manager_char: &Option<ManagerHoldingChar>,
) -> (u32, Vec<(String, u32)>) {
    let fund_type = &detail.fund_type;
    let is_bond = fund_type.contains("债券") || fund_type.contains("债");

    let return_s = score_return(periods, risk_metrics);
    let risk_s = score_risk(risk_metrics, fund_type);
    let stability_s = score_stability(risk_metrics, yearly_returns);
    let fee_s = score_fees(detail);
    let scale_s = score_scale(detail);
    let manager_s = score_manager(manager_eval);
    let style_s = score_holding_style(manager_char);

    let weights: Vec<(&str, u32, u32)> = if is_bond {
        vec![
            ("收益", return_s, 15),
            ("风险", risk_s, 30),
            ("稳定", stability_s, 20),
            ("费用", fee_s, 15),
            ("规模", scale_s, 10),
            ("经理", manager_s, 5),
            ("风格", style_s, 5),
        ]
    } else {
        vec![
            ("收益", return_s, 25),
            ("风险", risk_s, 30),
            ("稳定", stability_s, 15),
            ("费用", fee_s, 10),
            ("规模", scale_s, 10),
            ("经理", manager_s, 5),
            ("风格", style_s, 5),
        ]
    };

    let total: u32 = weights.iter().map(|(_, s, w)| s * w).sum();
    let overall = total / 100;
    let details = weights.into_iter().map(|(n, s, _)| (n.to_string(), s)).collect();
    (overall, details)
}
