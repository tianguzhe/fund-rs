use crate::ui;
use anyhow::Result;
use fund_core::api::Client;

pub fn run(client: &Client, keyword: &str) -> Result<()> {
    let funds = client.search_fund(keyword)?;
    ui::display_search_results(&funds);
    Ok(())
}
