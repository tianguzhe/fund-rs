use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Holding entry loaded from external config (JSON).
/// Keep field set minimal — runtime augments with API-fetched data.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HoldingEntry {
    pub code: String,
    pub name: String,
    pub amount: f64,
    pub channel: Option<String>,
    pub redeemable_date: Option<String>,
    pub redeem_status: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct HoldingsFile {
    holdings: Vec<HoldingEntry>,
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

/// Load holdings from the resolved config path. Returns a helpful error
/// pointing at the missing file rather than silently falling back to defaults
/// — silent fallback would mask a misplaced config and surprise the user.
pub fn load() -> Result<Vec<HoldingEntry>> {
    let path = config_path();
    if !path.exists() {
        anyhow::bail!(
            "持仓配置不存在: {}\n请运行 `fund holdings --init` 生成模板，或手动创建该文件",
            path.display()
        );
    }
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("读取持仓配置失败: {}", path.display()))?;
    let parsed: HoldingsFile = serde_json::from_str(&raw)
        .with_context(|| format!("解析持仓配置失败 (JSON 格式错误): {}", path.display()))?;
    if parsed.holdings.is_empty() {
        anyhow::bail!("持仓配置为空: {}", path.display());
    }
    Ok(parsed.holdings)
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
            HoldingEntry {
                code: "420002".into(),
                name: "天弘永利债A".into(),
                amount: 270000.0,
                channel: Some("招商".into()),
                redeemable_date: Some("2026-02-11".into()),
                redeem_status: Some("redeemable".into()),
            },
            HoldingEntry {
                code: "420002".into(),
                name: "天弘永利债A".into(),
                amount: 92119.0,
                channel: Some("支付宝".into()),
                redeemable_date: Some("2026-05-15".into()),
                redeem_status: Some("redeemable".into()),
            },
            HoldingEntry {
                code: "000171".into(),
                name: "易方达裕丰A".into(),
                amount: 150000.0,
                channel: Some("工商".into()),
                redeemable_date: Some("2026-05-08".into()),
                redeem_status: Some("redeemable".into()),
            },
        ],
    };
    let body = serde_json::to_string_pretty(&sample).context("序列化模板失败")?;
    std::fs::write(&path, body).with_context(|| format!("写入失败: {}", path.display()))?;
    Ok(path)
}
