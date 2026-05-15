use crate::ui;
use anyhow::Result;
use fund_core::api::Client;
use fund_core::f10;

pub fn run(client: &Client, code: &str) -> Result<()> {
    let detail = client.get_fund_estimate(code)?;
    let fee_rules = f10::get_fee_rules(code).ok();
    ui::display_fund_estimate(&detail, fee_rules.as_ref());

    let increases = client.get_period_increase(code)?;
    ui::display_period_increase(&increases);

    Ok(())
}
