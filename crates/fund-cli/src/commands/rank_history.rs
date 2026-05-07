use crate::ui;
use anyhow::Result;
use fund_core::api::Client;

pub fn run(client: &Client, code: &str, range: &str) -> Result<()> {
    let points = client.get_rank_history(code, range)?;
    ui::display_rank_history(&points, range);
    Ok(())
}
