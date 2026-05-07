use crate::ui;
use anyhow::Result;
use fund_core::api::Client;

pub fn run_list(client: &Client, category: i32) -> Result<()> {
    let items = client.get_big_data_list(category)?;
    ui::display_big_data_list(&items);
    Ok(())
}

pub fn run_detail(client: &Client, cltype: &str) -> Result<()> {
    let items = client.get_big_data_detail(cltype)?;
    ui::display_big_data_detail(&items);
    Ok(())
}
