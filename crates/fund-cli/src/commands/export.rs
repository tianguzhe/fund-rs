use anyhow::{Context, Result};
use fund_core::db;
use std::path::PathBuf;

pub fn run(output: &PathBuf) -> Result<()> {
    let value = db::export_json()?;
    let json = serde_json::to_string_pretty(&value).context("JSON serialization failed")?;
    std::fs::write(output, &json)?;
    println!("✓ 导出成功: {}", output.display());
    Ok(())
}
