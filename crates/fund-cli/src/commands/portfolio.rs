use anyhow::Result;
use fund_core::api::Client;
use fund_core::db::{self, DailyRecord};
use fund_core::holdings::{
    self, date_days, fetch_all_histories, period_return, profit_amount, MONTH_DAYS, WEEK_DAYS,
};
use fund_core::models::NetValuePoint;
use owo_colors::OwoColorize;
use unicode_width::UnicodeWidthStr;

const W_CODE: usize = 8;
const W_NAME: usize = 14;
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

// ── 主函数 ────────────────────────────────────────────────────────────

pub fn run(client: &Client, save: bool) -> Result<()> {
    let hold = holdings::holdings();
    let total: f64 = hold.iter().map(|h| h.amount).sum();

    let data: Vec<Option<Returns>> = fetch_all_histories(client, &hold)
        .into_iter()
        .map(|opt| opt.and_then(|pts| calc(&pts)))
        .collect();

    let line_w = 1 + W_CODE + 2 + W_NAME + 2 + W_AMT + 3 * (2 + W_PCT) + 3 + W_BAR + 7;
    let indent = 1 + W_CODE + 2 + W_NAME + 2 + W_AMT + 3;
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
        " {}  {}  {}   {}  {}  {}  {}",
        rpad("代码", W_CODE).bright_black().to_string(),
        rpad("基金名称", W_NAME).bright_black().to_string(),
        lpad("持仓(元)", W_AMT).bright_black().to_string(),
        lpad("当日", W_PCT).bright_black().to_string(),
        lpad("当周", W_PCT).bright_black().to_string(),
        lpad("当月", W_PCT).bright_black().to_string(),
        "仓位".bright_black(),
    );
    println!("{}", thin.bright_black());

    let (mut s_today, mut s_week, mut s_month) = (0.0f64, 0.0f64, 0.0f64);
    let mut save_records: Vec<DailyRecord> = Vec::new();

    for (h, r_opt) in hold.iter().zip(data.iter()) {
        let r = match r_opt {
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
                fund_code: h.code.to_string(),
                fund_name: h.name.to_string(),
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
            " {}  {}  {}   {}  {}  {}",
            rpad(h.code, W_CODE).bright_white().to_string(),
            rpad(h.name, W_NAME),
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
    }

    println!("{}", thin.bright_black());

    let r_today = s_today / total * 100.0;
    let r_week = s_week / total * 100.0;
    let r_month = s_month / total * 100.0;

    println!(
        " {}  {}  {}   {}  {}  {}",
        rpad("合计", W_CODE).bold().to_string(),
        rpad("", W_NAME),
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
    println!("{}", thick.bright_cyan());
    println!();

    if save {
        db::save_records(&save_records)?;
    }

    Ok(())
}
