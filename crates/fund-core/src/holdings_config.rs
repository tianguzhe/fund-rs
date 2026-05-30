use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Holding entry loaded from external config (JSON).
///
/// The user supplies `shares` (units held) and `cost_nav` (purchase NAV per
/// unit); market value and P&L are derived at runtime as `shares * nav`. This
/// keeps the config the single source of truth for *positions* while the DB
/// stores only daily snapshots. `redeemable_date` / `redeem_status` are lot
/// attributes used for display only — they never enter the DB.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HoldingEntry {
    pub code: String,
    pub name: String,
    /// Units held for this lot. Market value = `shares * nav`.
    pub shares: f64,
    /// Purchase NAV per unit. Holding-period return = `(nav - cost_nav) * shares`.
    pub cost_nav: f64,
    /// Purchase date (YYYY-MM-DD). Distinguishes lots of the *same* fund +
    /// channel bought at different times/prices (e.g. dollar-cost averaging),
    /// and is part of the `position_daily` lot key. Optional for back-compat;
    /// missing dates collapse to '' in the DB, so same-channel split lots
    /// should each carry one.
    pub buy_date: Option<String>,
    pub channel: Option<String>,
    pub redeemable_date: Option<String>,
    pub redeem_status: Option<String>,
}

/// A single cash movement. `amount` is signed: positive = money in
/// (redemption / dividend / deposit), negative = money out (subscription /
/// withdraw). Cash balance as of a date is the running sum of these.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CashFlow {
    pub date: String,
    pub amount: f64,
    /// Free-form category, e.g. "redeem" / "subscribe" / "dividend" /
    /// "deposit" / "withdraw". Not validated — used as a label only.
    pub flow_type: String,
    /// Optional related fund code (e.g. the fund a redemption came from).
    pub code: Option<String>,
    pub note: Option<String>,
}

/// Parsed config: positions plus the cash ledger. `cash_flows` defaults to
/// empty so configs without a cash section still load.
#[derive(Debug, Clone)]
pub struct HoldingsData {
    pub holdings: Vec<HoldingEntry>,
    pub cash_flows: Vec<CashFlow>,
}

#[derive(Debug, Deserialize, Serialize)]
struct HoldingsFile {
    holdings: Vec<HoldingEntry>,
    #[serde(default)]
    cash_flows: Vec<CashFlow>,
}

/// Resolve config path with priority:
/// 1. `$FUND_HOLDINGS` env var (absolute path, escape hatch for tests)
/// 2. `./holdings.json` in current working directory (project-local)
/// 3. `~/.fund-rs/holdings.json` (user-global default)
pub fn config_path() -> PathBuf {
    if let Ok(p) = std::env::var("FUND_HOLDINGS") {
        return PathBuf::from(p);
    }
    let cwd_local = PathBuf::from("holdings.json");
    if cwd_local.exists() {
        return cwd_local;
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".fund-rs").join("holdings.json")
}

/// Load holdings + cash ledger from the resolved config path. Returns a helpful
/// error pointing at the missing file rather than silently falling back to
/// defaults — silent fallback would mask a misplaced config and surprise the
/// user. The old `amount`-based format is intentionally incompatible: serde
/// will fail on the missing `shares` field rather than silently misvalue the
/// portfolio.
pub fn load() -> Result<HoldingsData> {
    let path = config_path();
    if !path.exists() {
        anyhow::bail!(
            "持仓配置不存在: {}\n请运行 `fund holdings --init` 生成模板，或手动创建该文件",
            path.display()
        );
    }
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("读取持仓配置失败: {}", path.display()))?;
    let parsed: HoldingsFile = serde_json::from_str(&raw).with_context(|| {
        format!(
            "解析持仓配置失败 (JSON 格式错误，或仍是旧 amount 格式): {}\n\
             新格式每笔需 shares + cost_nav 字段",
            path.display()
        )
    })?;
    if parsed.holdings.is_empty() {
        anyhow::bail!("持仓配置为空: {}", path.display());
    }
    Ok(HoldingsData { holdings: parsed.holdings, cash_flows: parsed.cash_flows })
}

/// Write a starter template to `~/.fund-rs/holdings.json` with example entries.
/// Refuses to overwrite an existing file — protects real holdings from accidental clobber.
pub fn init_template(target: Option<&Path>) -> Result<PathBuf> {
    let path = target.map(|p| p.to_path_buf()).unwrap_or_else(config_path);
    if path.exists() {
        anyhow::bail!("已存在: {}（如需覆盖请手动删除）", path.display());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("创建目录失败: {}", parent.display()))?;
    }
    let sample = HoldingsFile {
        holdings: vec![
            // Two lots of the same fund + channel bought on different dates —
            // distinct buy_date keeps them separate (split-lot support).
            HoldingEntry {
                code: "000171".into(),
                name: "易方达裕丰A".into(),
                shares: 60000.0,
                cost_nav: 1.4850,
                buy_date: Some("2026-05-08".into()),
                channel: Some("招商".into()),
                redeemable_date: Some("2026-05-08".into()),
                redeem_status: Some("redeemable".into()),
            },
            HoldingEntry {
                code: "000171".into(),
                name: "易方达裕丰A".into(),
                shares: 60000.0,
                cost_nav: 1.4960,
                buy_date: Some("2026-05-15".into()),
                channel: Some("招商".into()),
                redeemable_date: Some("2026-05-15".into()),
                redeem_status: Some("redeemable".into()),
            },
            HoldingEntry {
                code: "020359".into(),
                name: "东方红慧鑫C".into(),
                shares: 160000.0,
                cost_nav: 1.0310,
                buy_date: Some("2026-03-04".into()),
                channel: Some("招商".into()),
                redeemable_date: None,
                redeem_status: None,
            },
        ],
        cash_flows: vec![CashFlow {
            date: "2026-05-27".into(),
            amount: 89571.0,
            flow_type: "redeem".into(),
            code: Some("420002".into()),
            note: Some("420002 全部赎回".into()),
        }],
    };
    let body = serde_json::to_string_pretty(&sample).context("序列化模板失败")?;
    std::fs::write(&path, body).with_context(|| format!("写入失败: {}", path.display()))?;
    Ok(path)
}
