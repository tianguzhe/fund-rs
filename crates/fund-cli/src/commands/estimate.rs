//! `fund estimate` —— 基金盘中实时估值。
//!
//! 三种形态：
//! - 单只：`-c 161725`
//! - 多只：`-c 000171,161725`（逗号分隔）
//! - 持仓批量：省略 `-c` 时读 holdings.json，按份额估算今日盈亏
//!
//! 数据源为 `fund_core::realtime`（fundgz 直连），与统一 action API 无关。

use anyhow::{anyhow, Context, Result};
use serde::Serialize;

use fund_core::holdings;
use fund_core::realtime::{self, RealtimeEstimate};

use crate::ui;

/// 单只拉取结果：基金代码 + 可选持仓份额（仅持仓模式有）+ 估值结果。
type FetchResult = (String, Option<f64>, Result<RealtimeEstimate>);

pub fn run(codes: Option<&str>, json: bool) -> Result<()> {
    let targets = resolve_targets(codes)?;
    let holding_mode = codes.is_none();

    // 并发拉取每只基金的实时估值；单只失败不影响其他（结果各自保留 Result）。
    let results: Vec<FetchResult> = std::thread::scope(|scope| {
        let handles: Vec<_> = targets
            .iter()
            .map(|(code, shares)| {
                scope.spawn(move || (code.clone(), *shares, realtime::get_realtime_estimate(code)))
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });

    if json {
        return print_json(&results);
    }

    let mut rows: Vec<(RealtimeEstimate, Option<f64>)> = Vec::new();
    let mut failed: Vec<(String, String)> = Vec::new();
    for (code, shares, res) in results {
        match res {
            Ok(est) => rows.push((est, shares)),
            Err(e) => failed.push((code, e.to_string())),
        }
    }
    ui::display_realtime_estimates(&rows, &failed, holding_mode);
    Ok(())
}

/// 解析查询目标。`Some(codes)` 走查询模式（份额 None）；`None` 读持仓并按 code 合并份额。
fn resolve_targets(codes: Option<&str>) -> Result<Vec<(String, Option<f64>)>> {
    match codes {
        Some(s) => {
            let list: Vec<(String, Option<f64>)> = s
                .split(',')
                .map(str::trim)
                .filter(|c| !c.is_empty())
                .map(|c| (c.to_string(), None))
                .collect();
            if list.is_empty() {
                return Err(anyhow!("未提供有效基金代码"));
            }
            Ok(list)
        }
        None => {
            let hold = holdings::holdings()?;
            if hold.is_empty() {
                return Err(anyhow!(
                    "持仓为空，请先运行 `fund holdings --init` 配置 holdings.json"
                ));
            }
            // 同一基金代码的多笔（不同渠道 / buy_date 批次）份额求和，合并为一行。
            // 用 position() 而非 iter_mut().find()：后者返回的 &mut 借用会与 None 分支的
            // push 在 stable 借用检查器下冲突。
            let mut merged: Vec<(String, f64)> = Vec::new();
            for h in &hold {
                if let Some(pos) = merged.iter().position(|(c, _)| c == &h.code) {
                    merged[pos].1 += h.shares;
                } else {
                    merged.push((h.code.clone(), h.shares));
                }
            }
            Ok(merged.into_iter().map(|(c, s)| (c, Some(s))).collect())
        }
    }
}

fn print_json(results: &[FetchResult]) -> Result<()> {
    #[derive(Serialize)]
    struct EstOut<'a> {
        #[serde(flatten)]
        est: &'a RealtimeEstimate,
        #[serde(skip_serializing_if = "Option::is_none")]
        shares: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        est_market_value: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        est_pnl: Option<f64>,
    }
    #[derive(Serialize)]
    struct FailOut<'a> {
        code: &'a str,
        error: String,
    }
    #[derive(Serialize)]
    struct Root<'a> {
        estimates: Vec<EstOut<'a>>,
        failed: Vec<FailOut<'a>>,
    }

    let mut estimates = Vec::new();
    let mut failed = Vec::new();
    for (code, shares, res) in results {
        match res {
            Ok(est) => {
                // 持仓模式：估算市值 = 份额 × 估算净值；今日估算盈亏 = 份额 ×（估算净值 − 上一日净值）。
                let (mv, pnl) = match shares {
                    Some(s) => (Some(s * est.est_nav), Some(s * (est.est_nav - est.prev_nav))),
                    None => (None, None),
                };
                estimates.push(EstOut { est, shares: *shares, est_market_value: mv, est_pnl: pnl });
            }
            Err(e) => failed.push(FailOut { code, error: e.to_string() }),
        }
    }

    let root = Root { estimates, failed };
    let out = serde_json::to_string_pretty(&root).context("estimate JSON 序列化失败")?;
    println!("{out}");
    Ok(())
}
