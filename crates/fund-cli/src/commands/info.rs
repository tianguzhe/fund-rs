use crate::ui;
use anyhow::Result;
use fund_core::api::Client;

pub fn run(client: &Client, code: &str) -> Result<()> {
    let detail = client.get_fund_estimate(code)?;
    ui::display_fund_estimate(&detail);
    Ok(())
}
