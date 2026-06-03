use anyhow::Result;
use fund_core::api::Client;
use fund_core::db::{self, CashFlowInput, PositionSnapshot};
use fund_core::holdings::{
    self, classify, date_days, market_value, period_return, profit_amount,
    Holding, HISTORY_DAYS, MONTH_DAYS, WEEK_DAYS,
};
use fund_core::models::NetValuePoint;
use std::collections::BTreeMap;
use unicode_width::UnicodeWidthStr;

// ── 显示工具 ──────────────────────────────────────────────────────────

fn rpad(s: &str, width: usize) -> String {
    let w = UnicodeWidthStr::width(s);
    if w >= width {
        s.to_string()
    } else {
        format!("{}{}", s, " ".repeat(width - w))
    }
}

// ── 收益计算 ───────────────────────────────────────────────────────────

/// 重试辅助函数：指数退避策略 (100ms, 200ms, 400ms)
fn retry_with_backoff<T, F>(mut f: F, max_retries: usize) -> Option<T>
where
    F: FnMut() -> Option<T>,
{
    for attempt in 0..=max_retries {
        if let Some(result) = f() {
            return Some(result);
        }
        if attempt < max_retries {
            let delay_ms = 100 * (1 << attempt); // 100, 200, 400
            std::thread::sleep(std::time::Duration::from_millis(delay_ms));
        }
    }
    None
}

struct Returns {
    date: String,
    today: f64,
    week: f64,
    month: f64,
    nav: f64,
    acc_nav: f64,
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
        nav,
        acc_nav: latest.acc_value,
    })
}

struct Row {
    returns: Option<Returns>,
    fund_type: String,
}

/// 并发拉取每只持仓的：历史净值（用于近 1d/1w/1m）+ 详情（用于类型）
fn fetch_rows(client: &Client, hold: &[Holding]) -> Vec<Row> {
    std::thread::scope(|s| {
        let handles: Vec<_> = hold
            .iter()
            .map(|h| {
                s.spawn(|| {
                    // 重试最多 3 次，指数退避
                    let returns = retry_with_backoff(
                        || {
                            client
                                .get_net_value_history(&h.code, HISTORY_DAYS)
                                .ok()
                                .and_then(|pts| calc(&pts))
                        },
                        3,
                    );

                    let fund_type =
                        client.get_fund_estimate(&h.code).map(|d| d.fund_type).unwrap_or_default();

                    Row { returns, fund_type }
                })
            })
            .collect();
        handles.into_iter().map(|t| t.join().unwrap()).collect()
    })
}

// ── 主函数 ────────────────────────────────────────────────────────────

pub fn run(client: &Client, save: bool) -> Result<()> {
    let (hold, cash_flows) = holdings::portfolio_config()?;

    let data = fetch_rows(client, &hold);

    // Market value per lot needs NAV, so totals are computed after fetch.
    // Cash balance = sum of all configured flows (already-happened movements).
    let market_values: Vec<f64> = hold
        .iter()
        .zip(data.iter())
        .map(|(h, row)| row.returns.as_ref().map_or(0.0, |r| market_value(h.shares, r.nav)))
        .collect();
    let total_mv: f64 = market_values.iter().sum();
    let cash: f64 = cash_flows.iter().map(|c| c.amount).sum();
    let total_assets = total_mv + cash;

    // 计算总手续费
    let total_fee: f64 = hold.iter().map(|h| h.fee.unwrap_or(0.0)).sum();

    let (mut s_today, mut s_week, mut s_month) = (0.0f64, 0.0f64, 0.0f64);
    let mut save_records: Vec<PositionSnapshot> = Vec::new();

    // 资产配置聚合：类型 → 市值
    let mut allocation: BTreeMap<&'static str, f64> = BTreeMap::new();
    // 按基金代码分组，用于合并显示与计算持有期收益
    let mut code_groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, h) in hold.iter().enumerate() {
        code_groups.entry(h.code.clone()).or_insert_with(Vec::new).push(i);
    }

    // 预计算每个基金代码的合并数据（市值、收益、持有期）
    struct CodeAgg {
        name: String,
        asset_class: &'static str,
        total_mv: f64,
        total_cost: f64,
        p_today: f64,
        p_week: f64,
        p_month: f64,
    }
    let mut code_agg_map: BTreeMap<String, CodeAgg> = BTreeMap::new();

    for (i, (h, row)) in hold.iter().zip(data.iter()).enumerate() {
        let asset_class = classify(&row.fund_type);
        let mv = market_values[i];
        *allocation.entry(asset_class).or_insert(0.0) += mv;

        let r = match &row.returns {
            Some(r) => r,
            None => {
                eprintln!(" ⚠  {} 数据获取失败", h.code);
                continue;
            }
        };

        let p_today = profit_amount(mv, r.today);
        let p_week = profit_amount(mv, r.week);
        let p_month = profit_amount(mv, r.month);
        let cost = h.shares * h.cost_nav + h.fee.unwrap_or(0.0);

        s_today += p_today;
        s_week += p_week;
        s_month += p_month;

        // 聚合同代码的数据
        code_agg_map
            .entry(h.code.clone())
            .and_modify(|agg| {
                agg.total_mv += mv;
                agg.total_cost += cost;
                agg.p_today += p_today;
                agg.p_week += p_week;
                agg.p_month += p_month;
            })
            .or_insert(CodeAgg {
                name: h.name.clone(),
                asset_class,
                total_mv: mv,
                total_cost: cost,
                p_today,
                p_week,
                p_month,
            });

        if save {
            save_records.push(PositionSnapshot {
                date: r.date.clone(),
                code: h.code.clone(),
                name: h.name.clone(),
                fund_type: if row.fund_type.is_empty() {
                    None
                } else {
                    Some(row.fund_type.clone())
                },
                channel: h.channel.clone().unwrap_or_default(),
                buy_date: h.buy_date.clone().unwrap_or_default(),
                shares: h.shares,
                cost_nav: h.cost_nav,
                nav: r.nav,
                acc_nav: Some(r.acc_nav),
                growth: r.today,
            });
        }
    }

    // ── 输出：顶部总资产 + 现金 + 手续费 ──
    let nav_date = data.iter().find_map(|r| r.returns.as_ref().map(|r| r.date.as_str())).unwrap_or("--");
    println!();
    if total_fee > 0.0 {
        println!("{} 持仓总览（已扣 {:.2} 元手续费）", nav_date, total_fee);
    } else {
        println!("{} 持仓总览", nav_date);
    }
    println!();
    println!("  总资产：{:.0} 元", total_assets);
    // 避免显示 -0
    let cash_display = if cash.abs() < 0.01 { 0.0 } else { cash };
    println!("  现金：{:.0} 元", cash_display);
    println!();

    // ── 按类型分组输出表格 ──
    let mut type_order = vec!["债券", "混合", "股票", "指数", "QDII", "货币"];
    type_order.retain(|t| allocation.contains_key(t));

    for asset_type in type_order {
        let type_mv = allocation.get(asset_type).copied().unwrap_or(0.0);
        let type_pct = if total_assets > 0.0 { type_mv / total_assets * 100.0 } else { 0.0 };
        println!("  ---");
        println!("  {}型基金（{:.2}%，{:.0} 元）", asset_type, type_pct, type_mv);
        println!();

        // 表头
        println!(
            "  ┌────────┬─────────────────────┬──────────┬────────┬──────────┬────────┬─────────┬────────┬───────────┬───────────────────┐"
        );
        println!(
            "  │  代码  │     基金名称        │ 市值(元) │  1日   │ 1日盈亏  │  7日   │ 7日盈亏 │  30日  │ 30日盈亏  │    持有期收益     │"
        );
        println!(
            "  ├────────┼─────────────────────┼──────────┼────────┼──────────┼────────┼─────────┼────────┼───────────┼───────────────────┤"
        );

        // 筛选该类型的基金代码，按市值降序
        let mut codes_in_type: Vec<(&String, &CodeAgg)> = code_agg_map
            .iter()
            .filter(|(_, agg)| agg.asset_class == asset_type)
            .collect();
        codes_in_type.sort_by(|a, b| b.1.total_mv.partial_cmp(&a.1.total_mv).unwrap());

        for (code, agg) in codes_in_type {
            let r_today = if agg.total_mv > 0.0 {
                agg.p_today / agg.total_mv * 100.0
            } else {
                0.0
            };
            let r_week = if agg.total_mv > 0.0 {
                agg.p_week / agg.total_mv * 100.0
            } else {
                0.0
            };
            let r_month = if agg.total_mv > 0.0 {
                agg.p_month / agg.total_mv * 100.0
            } else {
                0.0
            };
            let hold_pnl = agg.total_mv - agg.total_cost;
            let hold_pct = if agg.total_cost > 0.0 {
                (agg.total_mv / agg.total_cost - 1.0) * 100.0
            } else {
                0.0
            };

            println!(
                "  │ {} │ {} │ {:>8.0} │ {:>6.2}% │ {:>8.0}元 │ {:>6.2}% │ {:>7.0}元 │ {:>6.2}% │ {:>9.0}元 │ {:>6.2}% ({:>7.0}元) │",
                rpad(code, 6),
                rpad(&agg.name, 19),
                agg.total_mv,
                r_today, agg.p_today,
                r_week, agg.p_week,
                r_month, agg.p_month,
                hold_pct, hold_pnl
            );
        }

        println!(
            "  └────────┴─────────────────────┴──────────┴────────┴──────────┴────────┴─────────┴────────┴───────────┴───────────────────┘"
        );
        println!();
    }

    // ── 收益汇总表 ──
    let r_today = if total_mv > 0.0 { s_today / total_mv * 100.0 } else { 0.0 };
    let r_week = if total_mv > 0.0 { s_week / total_mv * 100.0 } else { 0.0 };
    let r_month = if total_mv > 0.0 { s_month / total_mv * 100.0 } else { 0.0 };

    let total_cost: f64 = hold
        .iter()
        .map(|h| h.shares * h.cost_nav + h.fee.unwrap_or(0.0))
        .sum();
    let hold_pnl = total_mv - total_cost;
    let hold_pct = if total_cost > 0.0 { (total_mv / total_cost - 1.0) * 100.0 } else { 0.0 };

    println!("  ---");
    println!("  💰 收益汇总");
    println!();
    println!("  ┌────────┬────────┬────────────┬──────────────┐");
    println!("  │  周期  │ 收益率 │  盈亏金额  │     说明     │");
    println!("  ├────────┼────────┼────────────┼──────────────┤");
    println!(
        "  │ 1日    │ {:>6.2}% │ {:>10.0} 元  │ 今天         │",
        r_today, s_today
    );
    println!(
        "  │ 7日    │ {:>6.2}% │ {:>10.0} 元  │ 最近7天      │",
        r_week, s_week
    );
    println!(
        "  │ 30日   │ {:>6.2}% │ {:>10.0} 元 │ 最近30天     │",
        r_month, s_month
    );
    println!(
        "  │ 持有期 │ {:>6.2}% │ {:>10.0} 元  │ 扣费后净收益 │",
        hold_pct, hold_pnl
    );
    println!("  └────────┴────────┴────────────┴──────────────┘");
    println!();

    if save {
        let flows: Vec<CashFlowInput> = cash_flows
            .iter()
            .map(|c| CashFlowInput {
                date: c.date.clone(),
                amount: c.amount,
                flow_type: c.flow_type.clone(),
                code: c.code.clone(),
                note: c.note.clone(),
            })
            .collect();
        db::save_snapshot(&save_records, &flows)?;
    }

    Ok(())
}
