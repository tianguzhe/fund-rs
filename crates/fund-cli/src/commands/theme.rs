use crate::ui;
use anyhow::Result;
use fund_core::api::Client;

pub fn run(client: &Client, limit: usize) -> Result<()> {
    let themes = client.get_theme_list()?;
    ui::display_theme_list(&themes, limit);
    Ok(())
}
