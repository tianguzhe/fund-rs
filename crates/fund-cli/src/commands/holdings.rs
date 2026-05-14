use anyhow::{Context, Result};
use fund_core::api::Client;
use fund_core::f10::{self, IndustryAllocation, StockHolding};
use fund_core::holdings::{self, classify, Holding};
use fund_core::holdings_config;
use owo_colors::OwoColorize;
use serde::Serialize;
use std::collections::HashMap;
use unicode_width::UnicodeWidthStr;

/// 该基金是否可能持有股票（决定是否拉 F10）。
/// 关键例外：二级债基（FTYPE = "债券型-混合二级"）允许 ≤ 20% 股票仓位，
/// 必须做穿透才能反映真实暴露；因此只显式排除纯债/货币/短债品种。
fn has_stock_exposure(ftype: &str) -> bool {
    !(ftype.contains("纯债")
        || ftype.contains("货币")
        || ftype.contains("短债")
        || ftype.contains("中短债")
        || ftype.contains("中长债"))
}

// ── 拉取每只基金的快照 ────────────────────────────────────────────────

#[derive(Serialize)]
struct FundSnapshot {
    code: String,
    name: String,
    amount: f64,
    /// 原始类型字符串（来自 FTYPE，含细分如 "债券型-混合二级"）
    fund_type: String,
    asset_class: &'static str,
    /// F10 前十大股票，仅对含股票暴露的基金请求
    top_stocks: Option<Vec<StockHolding>>,
    /// F10 行业配置（最近一期），仅对含股票暴露的基金请求
    industries: Option<Vec<IndustryAllocation>>,
    /// jjcc 报告期
    period: Option<String>,
    /// jjcc 截止日期
    end_date: Option<String>,
}

fn fetch_snapshots(client: &Client, hold: &[Holding], year: u32, month: u32) -> Vec<FundSnapshot> {
    std::thread::scope(|s| {
        let handles: Vec<_> = hold
            .iter()
            .map(|h| {
                s.spawn(move || {
                    let fund_type =
                        client.get_fund_estimate(&h.code).map(|d| d.fund_type).unwrap_or_default();
                    let asset_class = classify(&fund_type);

                    let (top_stocks, period, end_date, industries) =
                        if has_stock_exposure(&fund_type) {
                            let inds = f10::get_active_industries(&h.code).ok();
                            match f10::get_top_stocks(&h.code, year, month).ok() {
                                Some(r) => (Some(r.stocks), Some(r.period), Some(r.end_date), inds),
                                None => (None, None, None, inds),
                            }
                        } else {
                            (None, None, None, None)
                        };

                    FundSnapshot {
                        code: h.code.clone(),
                        name: h.name.clone(),
                        amount: h.amount,
                        fund_type,
                        asset_class,
                        top_stocks,
                        industries,
                        period,
                        end_date,
                    }
                })
            })
            .collect();
        handles.into_iter().map(|t| t.join().unwrap()).collect()
    })
}

// ── 聚合 ──────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct StockExposure {
    code: String,
    name: String,
    /// 穿透到组合中的金额（元）
    amount: f64,
    /// 占整组合的比例 (%)
    weight: f64,
    /// 持有该股票的基金数
    fund_count: usize,
}

#[derive(Serialize)]
struct IndustryExposure {
    code: String,
    name: String,
    amount: f64,
    weight: f64,
    fund_count: usize,
}

#[derive(Serialize)]
struct AssetAllocation {
    asset_class: String,
    amount: f64,
    weight: f64,
    fund_count: usize,
}

#[derive(Serialize)]
struct HoldingsReport {
    generated_at: String,
    total_amount: f64,
    quarter: String,
    funds: Vec<FundSnapshot>,
    allocation: Vec<AssetAllocation>,
    top_stocks: Vec<StockExposure>,
    industries: Vec<IndustryExposure>,
    notes: Vec<String>,
}

fn build_report(snapshots: Vec<FundSnapshot>, top: usize, year: u32, month: u32) -> HoldingsReport {
    let total: f64 = snapshots.iter().map(|s| s.amount).sum();

    // 资产配置
    let mut alloc_map: HashMap<&'static str, (f64, usize)> = HashMap::new();
    for s in &snapshots {
        let e = alloc_map.entry(s.asset_class).or_insert((0.0, 0));
        e.0 += s.amount;
        e.1 += 1;
    }
    let mut allocation: Vec<AssetAllocation> = alloc_map
        .into_iter()
        .map(|(k, (amount, n))| AssetAllocation {
            asset_class: k.to_string(),
            amount,
            weight: if total > 0.0 { amount / total * 100.0 } else { 0.0 },
            fund_count: n,
        })
        .collect();
    allocation.sort_by(|a, b| b.amount.partial_cmp(&a.amount).unwrap_or(std::cmp::Ordering::Equal));

    // 股票穿透：(code, name) → (amount, fund_count)
    let mut stock_map: HashMap<String, (String, f64, usize)> = HashMap::new();
    for s in &snapshots {
        if let Some(stocks) = &s.top_stocks {
            for st in stocks {
                let amount = s.amount * st.ratio / 100.0;
                let e = stock_map
                    .entry(st.stock_code.clone())
                    .or_insert_with(|| (st.stock_name.clone(), 0.0, 0));
                e.1 += amount;
                e.2 += 1;
            }
        }
    }
    let mut top_stocks: Vec<StockExposure> = stock_map
        .into_iter()
        .map(|(code, (name, amount, n))| StockExposure {
            code,
            name,
            amount,
            weight: if total > 0.0 { amount / total * 100.0 } else { 0.0 },
            fund_count: n,
        })
        .collect();
    top_stocks.sort_by(|a, b| b.amount.partial_cmp(&a.amount).unwrap_or(std::cmp::Ordering::Equal));
    top_stocks.truncate(top);

    // 行业穿透
    let mut ind_map: HashMap<String, (String, f64, usize)> = HashMap::new();
    for s in &snapshots {
        if let Some(inds) = &s.industries {
            for i in inds {
                let amount = s.amount * i.ratio / 100.0;
                let e = ind_map
                    .entry(i.industry_code.clone())
                    .or_insert_with(|| (i.industry_name.clone(), 0.0, 0));
                e.1 += amount;
                e.2 += 1;
            }
        }
    }
    let mut industries: Vec<IndustryExposure> = ind_map
        .into_iter()
        .map(|(code, (name, amount, n))| IndustryExposure {
            code,
            name,
            amount,
            weight: if total > 0.0 { amount / total * 100.0 } else { 0.0 },
            fund_count: n,
        })
        .collect();
    industries.sort_by(|a, b| b.amount.partial_cmp(&a.amount).unwrap_or(std::cmp::Ordering::Equal));

    // 注释：说明哪些基金未做穿透
    let mut notes = Vec::new();
    let skipped: Vec<&FundSnapshot> = snapshots.iter().filter(|s| s.top_stocks.is_none()).collect();
    if !skipped.is_empty() {
        notes.push(format!("已跳过 {} 只债/货基的股票穿透（暴露低、F10 不必要）", skipped.len()));
    }
    if top_stocks.is_empty() {
        notes.push("无股票穿透数据：当前组合无含股基金，或 F10 接口未返回".to_string());
    }

    let end_dates: Vec<&str> = snapshots.iter().filter_map(|s| s.end_date.as_deref()).collect();
    let generated_at = end_dates.iter().max().copied().unwrap_or("").to_string();

    HoldingsReport {
        generated_at,
        total_amount: total,
        quarter: format!("{}年{}季度", year, (month as i32 - 1) / 3 + 1),
        funds: snapshots,
        allocation,
        top_stocks,
        industries,
        notes,
    }
}

// ── 终端输出 ─────────────────────────────────────────────────────────

fn rpad(s: &str, width: usize) -> String {
    let w = UnicodeWidthStr::width(s);
    if w >= width {
        s.to_string()
    } else {
        format!("{}{}", s, " ".repeat(width - w))
    }
}

fn bar(pct: f64, w: usize) -> String {
    let filled = ((pct / 100.0) * w as f64).round() as usize;
    let filled = filled.min(w);
    format!("{}{}", "█".repeat(filled), "░".repeat(w - filled))
}

fn render_terminal(r: &HoldingsReport) {
    let total = r.total_amount;
    let thick = "━".repeat(70);
    let thin = "─".repeat(70);

    println!();
    println!("{}", thick.bright_cyan());
    println!(
        " {}  总资产: {}   报告期: {}",
        "组合穿透分析".bright_cyan().bold(),
        format!("{:.0} 元", total).yellow().bold(),
        r.quarter.bright_white(),
    );
    println!("{}", thick.bright_cyan());

    println!();
    println!(" {}", "资产配置".bright_white().bold());
    for a in &r.allocation {
        println!(
            "   {} {}  {:>12}  {:>6.2}%  ({} 只)",
            rpad(&a.asset_class, 6).bright_blue(),
            bar(a.weight, 18),
            format!("{:.0} 元", a.amount),
            a.weight,
            a.fund_count,
        );
    }

    if !r.top_stocks.is_empty() {
        println!();
        println!(" {} (按穿透金额排序)", "底层股票 TOP".bright_white().bold());
        println!(
            "   {} {} {} {} {}",
            rpad("代码", 8).bright_black(),
            rpad("名称", 14).bright_black(),
            rpad("穿透金额", 12).bright_black(),
            rpad("组合占比", 9).bright_black(),
            "重复持有".bright_black(),
        );
        println!("{}", thin.bright_black());
        for s in &r.top_stocks {
            println!(
                "   {} {} {:>12} {:>8.3}%  {}",
                rpad(&s.code, 8).bright_white(),
                rpad(&s.name, 14),
                format!("{:.0} 元", s.amount),
                s.weight,
                if s.fund_count > 1 {
                    format!("{} 只基金", s.fund_count).yellow().to_string()
                } else {
                    "—".bright_black().to_string()
                },
            );
        }
    }

    if !r.industries.is_empty() {
        println!();
        println!(" {} (按穿透金额排序)", "行业配置".bright_white().bold());
        for i in &r.industries {
            println!(
                "   {} {}  {:>12}  {:>6.3}%",
                rpad(&i.name, 28).bright_magenta(),
                bar(i.weight, 18),
                format!("{:.0} 元", i.amount),
                i.weight,
            );
        }
    }

    if !r.notes.is_empty() {
        println!();
        for n in &r.notes {
            println!(" {} {}", "•".bright_black(), n.bright_black());
        }
    }

    println!("{}", thick.bright_cyan());
    println!();
}

// ── 入口 ──────────────────────────────────────────────────────────────

pub fn run(client: &Client, top: usize, json: bool) -> Result<()> {
    let hold = holdings::holdings()?;

    let (year, month) = current_year_month();
    let (qy, qm) = f10::latest_quarter_end(year, month);

    eprintln!("拉取持仓快照（季度: {}-{:02}）...", qy, qm);
    let snapshots = fetch_snapshots(client, &hold, qy, qm);
    let report = build_report(snapshots, top, qy, qm);

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        render_terminal(&report);
    }
    Ok(())
}

pub fn init(force: bool) -> Result<()> {
    let path = holdings_config::config_path();
    if path.exists() && !force {
        eprintln!("已存在: {}（如需重新生成请删除该文件）", path.display());
        return Ok(());
    }
    if path.exists() && force {
        std::fs::remove_file(&path).with_context(|| format!("删除失败: {}", path.display()))?;
    }
    let p = holdings_config::init_template(None)?;
    println!("✓ 已生成持仓模板: {}", p.display());
    println!("  编辑该文件后再运行 `fund holdings` / `fund portfolio`");
    Ok(())
}

// ── 工具：从 epoch 秒计算 (year, month)；std::time 无日历 API，自行换算 ──

fn current_year_month() -> (u32, u32) {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    // 本地时区与 UTC 偏差最多 1 天，对季度判断无影响。
    let (y, m, _d) = days_to_ymd((secs / 86400) as i32);
    (y as u32, m as u32)
}

/// 1970-01-01 起算第 `days` 天 → (year, month, day). 基于公历，含闰年。
fn days_to_ymd(days: i32) -> (i32, i32, i32) {
    let mut y = 1970i32;
    let mut d = days;
    loop {
        let yd = if is_leap(y) { 366 } else { 365 };
        if d < yd {
            break;
        }
        d -= yd;
        y += 1;
    }
    let months = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut m = 0;
    let mut dd = d;
    while m < 12 {
        let md = if m == 1 && is_leap(y) { 29 } else { months[m] };
        if dd < md {
            break;
        }
        dd -= md;
        m += 1;
    }
    (y, (m + 1) as i32, dd + 1)
}

fn is_leap(y: i32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}
