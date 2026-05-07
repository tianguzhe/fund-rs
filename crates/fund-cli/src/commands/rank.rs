use crate::ui;
use anyhow::Result;
use fund_core::api::Client;
use fund_core::models::FundRankParams;

pub fn run(client: &Client, params: FundRankParams) -> Result<()> {
    let ranks = client.get_fund_rank(&params)?;
    ui::display_fund_rank(&ranks);
    Ok(())
}
