use anyhow::Result;
use fund_core::api::Client;
use fund_core::db::{self, DailyRecord};
use fund_core::holdings::{
    self, classify, date_days, period_return, profit_amount, Holding, HISTORY_DAYS, MONTH_DAYS,
    WEEK_DAYS,
};
use fund_core::models::NetValuePoint;
use owo_colors::OwoColorize;
use std::collections::BTreeMap;
use unicode_width::UnicodeWidthStr;

const W_CODE: usize = 8;
const W_NAME: usize = 14;
const W_CHANNEL: usize = 6;
const W_TYPE: usize = 6;
const W_AMT: usize = 10;
const W_PCT: usize = 8;
const W_YUAN: usize = 8;
const W_BAR: usize = 16;

// ── 显示工具 ──────────────────────────────────────────────────────────

fn rpad(s: &str, width: usize) -> String {
    let w = UnicodeWidthStr::width(s);
    if w >= width {
        s.to_string()
    } else {
        format!("{}{}", s, " ".repeat(width - w))
    }
}

fn lpad(s: &str, width: usize) -> String {
    let w = UnicodeWidthStr::width(s);
    if w >= width {
        s.to_string()
    } else {
        format!("{}{}", " ".repeat(width - w), s)
    }
}

fn colorize(v: f64, s: &str) -> String {
    if v > 0.0 {
        s.green().to_string()
    } else if v < 0.0 {
        s.red().to_string()
    } else {
        s.bright_black().to_string()
    }
}

fn fmt_value(v: f64, w: usize, decimals: usize, suffix: &str) -> String {
    let s = if v >= 0.0 {
        format!("+{:.prec$}{}", v, suffix, prec = decimals)
    } else {
        format!("{:.prec$}{}", v, suffix, prec = decimals)
    };
    colorize(v, &lpad(&s, w))
}

fn fmt_pct(v: f64) -> String {
    fmt_value(v, W_PCT, 2, "%")
}
fn fmt_yuan(v: f64) -> String {
    fmt_value(v, W_YUAN, 0, "元")
}

fn progress_bar(pct: f64, w: usize) -> String {
    let filled = ((pct / 100.0) * w as f64).round() as usize;
    let filled = filled.min(w);
    format!("{}{}", "█".repeat(filled), "░".repeat(w - filled))
}

// ── 收益计算 ───────────────────────────────────────────────────────────

struct Returns {
    date: String,
    today: f64,
    week: f64,
    month: f64,
}

fn calc(points: &[NetValuePoint]) -> Option<Returns> {
    let latest = points.first()?;
    let d0 = date_days(&latest.date)?;
    let nav = latest.net_value;
    Some(Returns {
        date: latest.date.clone(),
        today: latest.growth,
        week: period_return(points, nav, d0, WEEK_DAYS),
        month: period_return(points, nav, d0, MONTH_DAYS),
    })
}

struct Row {
    returns: Option<Returns>,
    fund_type: String,
    /// 盘中估值涨跌 (%) 与时间。债基/货基常为 None。
    estimation: Option<(f64, String)>,
    /// 申购状态文案（如 "开放申购"/"暂停申购"），为空表示接口未返回。
    buy_status: Option<String>,
}

/// 并发拉取每只持仓的：历史净值（用于近 1d/1w/1m）+ 详情（用于类型）+ 盘中估值（可选）。
fn fetch_rows(client: &Client, hold: &[Holding]) -> Vec<Row> {
    std::thread::scope(|s| {
        let handles: Vec<_> = hold
            .iter()
            .map(|h| {
                s.spawn(|| {
                    let returns = client
                        .get_net_value_history(&h.code, HISTORY_DAYS)
                        .ok()
                        .and_then(|pts| calc(&pts));

                    let fund_type =
                        client.get_fund_estimate(&h.code).map(|d| d.fund_type).unwrap_or_default();

                    let (estimation, buy_status) = match client.get_fund_estimation(&h.code) {
                        Ok(e) => {
                            let pct = e.change_pct.parse::<f64>().ok().map(|p| (p, e.time));
                            let buy =
                                if e.buy_status.is_empty() { None } else { Some(e.buy_status) };
                            (pct, buy)
                        }
                        Err(_) => (None, None),
                    };

                    Row { returns, fund_type, estimation, buy_status }
                })
            })
            .collect();
        handles.into_iter().map(|t| t.join().unwrap()).collect()
    })
}

// ── 主函数 ────────────────────────────────────────────────────────────

pub fn run(client: &Client, save: bool) -> Result<()> {
    let hold = holdings::holdings()?;
    let total: f64 = hold.iter().map(|h| h.amount).sum();

    let data = fetch_rows(client, &hold);

    let line_w = 1 + W_CODE + 2 + W_NAME + 2 + W_CHANNEL + 2 + W_TYPE + 2 + W_AMT
        + 3 * (2 + W_PCT) + 3 + W_BAR + 7;
    let indent = 1 + W_CODE + 2 + W_NAME + 2 + W_CHANNEL + 2 + W_TYPE + 2 + W_AMT + 3;
    let thick = "━".repeat(line_w);
    let thin = "─".repeat(line_w);

    println!();
    println!("{}", thick.bright_cyan());
    println!(
        " {}  总资产: {}",
        "持仓概览".bright_cyan().bold(),
        format!("{:.0} 元", total).yellow().bold()
    );
    println!("{}", thick.bright_cyan());
    println!(
        " {}  {}  {}  {}  {}   {}  {}  {}  {}",
        rpad("代码", W_CODE).bright_black(),
        rpad("基金名称", W_NAME).bright_black(),
        rpad("渠道", W_CHANNEL).bright_black(),
        rpad("类型", W_TYPE).bright_black(),
        lpad("持仓(元)", W_AMT).bright_black(),
        lpad("当日", W_PCT).bright_black(),
        lpad("当周", W_PCT).bright_black(),
        lpad("当月", W_PCT).bright_black(),
        "仓位".bright_black(),
    );
    println!("{}", thin.bright_black());

    let (mut s_today, mut s_week, mut s_month) = (0.0f64, 0.0f64, 0.0f64);
    let mut save_records: Vec<DailyRecord> = Vec::new();

    // 资产配置聚合：类型 → 金额
    let mut allocation: BTreeMap<&'static str, f64> = BTreeMap::new();
    // 估值/申购状态辅助行
    let mut footnotes: Vec<String> = Vec::new();

    for (h, row) in hold.iter().zip(data.iter()) {
        let asset_class = classify(&row.fund_type);
        *allocation.entry(asset_class).or_insert(0.0) += h.amount;

        let r = match &row.returns {
            Some(r) => r,
            None => {
                eprintln!(" ⚠  {} 数据获取失败", h.code);
                continue;
            }
        };

        let weight = h.amount / total * 100.0;
        let p_today = profit_amount(h.amount, r.today);
        let p_week = profit_amount(h.amount, r.week);
        let p_month = profit_amount(h.amount, r.month);

        s_today += p_today;
        s_week += p_week;
        s_month += p_month;

        if save {
            save_records.push(DailyRecord {
                date: r.date.clone(),
                fund_code: h.code.clone(),
                fund_name: h.name.clone(),
                holding: h.amount,
                day_pct: r.today,
                day_amount: p_today,
                week_pct: r.week,
                week_amount: p_week,
                month_pct: r.month,
                month_amount: p_month,
            });
        }

        println!(
            " {}  {}  {}  {}  {}   {}  {}  {}",
            rpad(&h.code, W_CODE).bright_white(),
            rpad(&h.name, W_NAME),
            rpad(h.channel.as_deref().unwrap_or("-"), W_CHANNEL).bright_black(),
            rpad(asset_class, W_TYPE).bright_blue(),
            lpad(&format!("{:.0}", h.amount), W_AMT),
            fmt_pct(r.today),
            fmt_pct(r.week),
            fmt_pct(r.month),
        );
        println!(
            "{}{}  {}  {}   {} {:.1}%",
            " ".repeat(indent),
            fmt_yuan(p_today),
            fmt_yuan(p_week),
            fmt_yuan(p_month),
            progress_bar(weight, W_BAR),
            weight,
        );
        println!();

        // 收集盘中估值/申购状态辅助信息（避免主表过宽）
        if let Some((pct, time)) = &row.estimation {
            let buy = row.buy_status.as_deref().unwrap_or("");
            let suffix = if buy.is_empty() { String::new() } else { format!("  · {}", buy) };
            footnotes.push(format!(
                "  {} {}  估值 {} @ {}{}",
                h.code,
                h.name,
                fmt_value(*pct, 7, 2, "%"),
                time,
                suffix
            ));
        } else if let Some(buy) = &row.buy_status {
            if !buy.is_empty() {
                footnotes.push(format!("  {} {}  {}", h.code, h.name, buy));
            }
        }
    }

    println!("{}", thin.bright_black());

    let r_today = s_today / total * 100.0;
    let r_week = s_week / total * 100.0;
    let r_month = s_month / total * 100.0;

    println!(
        " {}  {}  {}  {}  {}   {}  {}  {}",
        rpad("合计", W_CODE).bold(),
        rpad("", W_NAME),
        rpad("", W_CHANNEL),
        rpad("", W_TYPE),
        lpad(&format!("{:.0}", total), W_AMT),
        fmt_pct(r_today),
        fmt_pct(r_week),
        fmt_pct(r_month),
    );
    println!(
        "{}{}  {}  {}",
        " ".repeat(indent),
        fmt_yuan(s_today),
        fmt_yuan(s_week),
        fmt_yuan(s_month),
    );

    // 资产配置摘要：按金额降序
    println!();
    println!(" {}", "资产配置".bright_cyan().bold());
    let mut alloc_sorted: Vec<(&&str, &f64)> = allocation.iter().collect();
    alloc_sorted.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap_or(std::cmp::Ordering::Equal));
    for (klass, amount) in alloc_sorted {
        let pct = amount / total * 100.0;
        println!(
            "   {} {} {:>10}  {:>6.2}%",
            rpad(klass, W_TYPE).bright_blue(),
            progress_bar(pct, W_BAR),
            format!("{:.0} 元", amount),
            pct,
        );
    }

    // 盘中估值/申购状态（仅在有数据时展示，避免对债基/货基输出大片空白）
    if !footnotes.is_empty() {
        println!();
        println!(" {}", "盘中估值 / 申购状态".bright_cyan().bold());
        for line in footnotes {
            println!("{}", line);
        }
    }

    println!("{}", thick.bright_cyan());
    println!();

    if save {
        db::save_records(&save_records)?;
    }

    Ok(())
}
