use crate::ui;
use anyhow::Result;
use fund_core::api::Client;

pub fn run(client: &Client, code: &str, days: i32, limit: usize) -> Result<()> {
    let points = client.get_net_value_history(code, days)?;
    ui::display_net_value_history(&points, limit);
    Ok(())
}
