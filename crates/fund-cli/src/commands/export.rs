use anyhow::{Context, Result};
use fund_core::db;
use std::path::PathBuf;

pub fn run(output: &PathBuf) -> Result<()> {
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent).context("Failed to create output directory")?;
    }
    let value = db::export_json()?;
    let json = serde_json::to_string_pretty(&value).context("JSON serialization failed")?;
    std::fs::write(output, &json)?;
    println!("✓ 导出成功: {}", output.display());
    Ok(())
}
