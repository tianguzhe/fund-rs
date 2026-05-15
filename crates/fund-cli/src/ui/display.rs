use comfy_table::{presets::UTF8_FULL, ContentArrangement, Table};
use owo_colors::OwoColorize;

use fund_core::f10::FeeRules;
use fund_core::models::*;

const SCALE_DIVISOR: f64 = 100_000_000.0;

fn create_styled_table() -> Table {
    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.set_content_arrangement(ContentArrangement::Dynamic);
    table
}

fn colorize_growth(value: f64, text: &str) -> String {
    if value > 0.0 {
        text.green().to_string()
    } else if value < 0.0 {
        text.red().to_string()
    } else {
        text.to_string()
    }
}

fn format_percentage(value: f64) -> String {
    let formatted = format!("{:.2}%", value);
    let formatted = if value > 0.0 { format!("+{}", formatted) } else { formatted };
    colorize_growth(value, &formatted)
}

fn parse_and_colorize_percentage(value_str: &str) -> String {
    let value: f64 = value_str.parse().unwrap_or(0.0);
    let formatted = format!("{}%", value_str);
    colorize_growth(value, &formatted)
}

pub fn display_search_results(funds: &[FundSearchResult]) {
    if funds.is_empty() {
        println!("No funds found");
        return;
    }

    println!("\nSearch Results:");
    let mut table = create_styled_table();
    table.set_header(vec!["Code", "Name", "Type"]);

    for fund in funds {
        table.add_row(vec![&fund.code, &fund.name, &fund.fund_type]);
    }

    println!("{}", table);
}

fn add_optional_row(table: &mut Table, label: &str, value: &str) {
    if !value.is_empty() {
        table.add_row(vec![label, value]);
    }
}

fn format_scale(scale_str: &str) -> Option<String> {
    scale_str.parse::<f64>().ok().map(|scale| format!("{:.2}", scale / SCALE_DIVISOR))
}

pub fn display_fund_estimate(detail: &FundDetail, fee_rules: Option<&FeeRules>) {
    println!("\nFund Information:");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    let mut table = create_styled_table();
    table.set_header(vec!["Field", "Value"]);

    table.add_row(vec!["Code", &detail.code]);
    table.add_row(vec!["Name", &detail.name]);
    add_optional_row(&mut table, "Full Name", &detail.full_name);
    table.add_row(vec!["Type", &detail.fund_type]);
    table.add_row(vec!["Established", &detail.estab_date]);
    table.add_row(vec!["Company", &detail.company]);
    table.add_row(vec!["Manager", &detail.manager]);
    add_optional_row(&mut table, "Custodian", &detail.custodian);

    if let Some(scale_in_billion) = format_scale(&detail.scale) {
        table.add_row(vec!["Scale (亿)", &scale_in_billion]);
    }

    add_optional_row(&mut table, "Risk Level", &detail.risk_level);
    add_optional_row(&mut table, "Mgmt Fee", &detail.mgr_fee);
    add_optional_row(&mut table, "Custody Fee", &detail.trust_fee);
    add_optional_row(&mut table, "Sales Fee", &detail.sales_fee);

    if let Some(rules) = fee_rules {
        if !rules.redemption.is_empty() {
            let redemption = rules
                .redemption
                .iter()
                .map(|rule| format!("{} {}", rule.scope, rule.rate))
                .collect::<Vec<_>>()
                .join(" | ");
            table.add_row(vec!["Redemption Fee", &redemption]);
        }
    }

    println!("{}", table);
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
}

fn format_rank(rank: i32, total: i32) -> String {
    if rank > 0 && total > 0 {
        format!("{}/{}", rank, total)
    } else {
        "-".to_string()
    }
}

pub fn display_period_increase(increases: &[PeriodIncrease]) {
    if increases.is_empty() {
        println!("No performance data available");
        return;
    }

    println!("\nPeriod Performance:");
    let mut table = create_styled_table();
    table.set_header(vec!["Period", "Return", "Avg", "HS300", "Rank"]);

    for increase in increases {
        let return_str = format_percentage(increase.return_rate);
        let avg_str = format_percentage(increase.avg);
        let hs300_str = format_percentage(increase.hs300_return);
        let rank_str = format_rank(increase.rank, increase.total);

        table.add_row(vec![increase.title.as_str(), &return_str, &avg_str, &hs300_str, &rank_str]);
    }

    println!("{}", table);
}

fn calculate_average_growth(points: &[NetValuePoint]) -> f64 {
    if points.is_empty() {
        return 0.0;
    }
    let total_growth: f64 = points.iter().map(|p| p.growth).sum();
    total_growth / points.len() as f64
}

pub fn display_net_value_history(points: &[NetValuePoint], limit: usize) {
    if points.is_empty() {
        println!("No historical data available");
        return;
    }

    let points = if limit > 0 && limit < points.len() { &points[..limit] } else { points };

    println!("\nNet Value History:");
    let mut table = create_styled_table();
    table.set_header(vec!["Date", "Net Value", "Acc Value", "Growth"]);

    for point in points {
        let growth_str = format_percentage(point.growth);

        table.add_row(vec![
            point.date.as_str(),
            &format!("{:.4}", point.net_value),
            &format!("{:.4}", point.acc_value),
            &growth_str,
        ]);
    }

    println!("{}", table);

    let avg_growth = calculate_average_growth(points);
    let avg_str = format_percentage(avg_growth);
    println!("\nStatistics: Average Growth {}", avg_str);
}

pub fn display_theme_list(themes: &[FundTheme], limit: usize) {
    if themes.is_empty() {
        println!("No themes available");
        return;
    }

    let themes = if limit > 0 && limit < themes.len() { &themes[..limit] } else { themes };

    println!("\nFund Themes:");
    let mut table = create_styled_table();
    table.set_header(vec!["Code", "Name", "Type"]);

    for theme in themes {
        let type_str = if theme.theme_type == "2" { "Concept" } else { "Industry" };
        table.add_row(vec![&theme.code, &theme.name, type_str]);
    }

    println!("{}", table);
}

pub fn display_big_data_list(items: &[BigDataItem]) {
    if items.is_empty() {
        println!("No data available");
        return;
    }

    println!("\nBig Data Ranking:");
    let mut table = create_styled_table();
    table.set_header(vec!["Title", "Fund Code", "Fund Name", "Return", "Period"]);

    for item in items {
        let return_str = parse_and_colorize_percentage(&item.return_rate);

        table.add_row(vec![
            item.title.as_str(),
            &item.fund_code,
            &item.fund_name,
            &return_str,
            &item.period,
        ]);
    }

    println!("{}", table);
}

pub fn display_fund_rank(ranks: &[FundRank]) {
    if ranks.is_empty() {
        println!("No ranking data available");
        return;
    }

    println!("\nFund Ranking:");
    let mut table = create_styled_table();
    table.set_header(vec!["Code", "Name", "Net Value", "Acc Value", "Week", "Month", "Year"]);

    for rank in ranks {
        let week_str = parse_and_colorize_percentage(&rank.week_growth);
        let month_str = parse_and_colorize_percentage(&rank.month_growth);

        let year_str = if rank.year_growth == "--" {
            rank.year_growth.clone()
        } else {
            parse_and_colorize_percentage(&rank.year_growth)
        };

        table.add_row(vec![
            &rank.code,
            &rank.name,
            &rank.net_value,
            &rank.acc_value,
            &week_str,
            &month_str,
            &year_str,
        ]);
    }

    println!("{}", table);
}

pub fn display_big_data_detail(items: &[BigDataDetailItem]) {
    if items.is_empty() {
        println!("No data available");
        return;
    }

    println!("\nBig Data Detail:");
    let mut table = create_styled_table();
    table.set_header(vec!["Code", "Name", "Net Value", "Acc Value", "Return"]);

    for item in items {
        let return_str = parse_and_colorize_percentage(&item.return_rate);

        table.add_row(vec![&item.code, &item.name, &item.net_value, &item.acc_value, &return_str]);
    }

    println!("{}", table);
}

fn get_rank_color(percentage: f64) -> impl Fn(&str) -> String {
    move |text: &str| {
        if percentage <= 20.0 {
            text.red().to_string()
        } else if percentage <= 50.0 {
            text.yellow().to_string()
        } else {
            text.green().to_string()
        }
    }
}

pub fn display_rank_history(points: &[RankHistoryPoint], range: &str) {
    if points.is_empty() {
        println!("No rank history data available");
        return;
    }

    println!("\n{}", "Rank History Chart".bright_cyan().bold());
    println!("Time Range: {}", range.yellow());
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    let data: Vec<(String, f64)> = points
        .iter()
        .filter_map(|p| {
            let percentage = p.rank_percentage()?;
            Some((p.date.clone(), percentage))
        })
        .collect();

    if data.is_empty() {
        println!("No valid rank data");
        return;
    }

    let sample_size = 80;
    let step = (data.len() as f64 / sample_size as f64).ceil() as usize;
    let sampled: Vec<_> = data.iter().step_by(step.max(1)).collect();

    let chart_height = 25;
    let chart_width = sampled.len();

    let min_pct = 0.0;
    let max_pct = 100.0;

    println!();
    for row in 0..=chart_height {
        let threshold = max_pct - (row as f64 * (max_pct - min_pct) / chart_height as f64);

        if row % 5 == 0 {
            print!("{:>5.0}% │", threshold);
        } else {
            print!("       │");
        }

        for (_, pct) in &sampled {
            let diff = (*pct - threshold).abs();
            let char = if diff < 2.0 {
                let color_fn = get_rank_color(*pct);
                color_fn("·")
            } else {
                " ".to_string()
            };
            print!("{}", char);
        }
        println!();
    }

    print!("       └");
    for _ in 0..chart_width {
        print!("─");
    }
    println!();

    if let (Some(first), Some(last)) = (sampled.first(), sampled.last()) {
        println!(
            "        {}  {}  {}",
            first.0.bright_black(),
            "→".bright_black(),
            last.0.bright_black()
        );
    }

    println!();

    let avg_pct = data.iter().map(|(_, p)| p).sum::<f64>() / data.len() as f64;
    let best_pct =
        data.iter().min_by(|a, b| a.1.partial_cmp(&b.1).unwrap()).map(|(_, p)| p).unwrap_or(&0.0);
    let worst_pct =
        data.iter().max_by(|a, b| a.1.partial_cmp(&b.1).unwrap()).map(|(_, p)| p).unwrap_or(&0.0);

    let excellent_days = data.iter().filter(|(_, p)| *p <= 20.0).count();
    let good_days = data.iter().filter(|(_, p)| *p > 20.0 && *p <= 50.0).count();
    let average_days = data.iter().filter(|(_, p)| *p > 50.0).count();
    let total_days = data.len();

    println!("{}", "Performance Statistics:".bright_cyan().bold());
    println!("  Average Rank:  {:.1}% {}", avg_pct, get_rank_color(avg_pct)("●"));
    println!(
        "  Best Rank:     {:.1}% {}",
        best_pct,
        get_rank_color(*best_pct)("● (Peak Performance)")
    );
    println!(
        "  Worst Rank:    {:.1}% {}",
        worst_pct,
        get_rank_color(*worst_pct)("● (Lowest Point)")
    );

    println!();
    println!("{}", "Time Distribution:".bright_cyan().bold());
    println!(
        "  {} Top 20%:     {} days ({:.1}%)",
        "●".red(),
        excellent_days,
        (excellent_days as f64 / total_days as f64) * 100.0
    );
    println!(
        "  {} 20%-50%:     {} days ({:.1}%)",
        "●".yellow(),
        good_days,
        (good_days as f64 / total_days as f64) * 100.0
    );
    println!(
        "  {} Bottom 50%:  {} days ({:.1}%)",
        "●".green(),
        average_days,
        (average_days as f64 / total_days as f64) * 100.0
    );

    println!();
    println!("{}", "Note:".bright_black());
    println!("  {}", "Lower percentage = Better ranking (e.g., 10% means top 10%)".bright_black());
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
}
