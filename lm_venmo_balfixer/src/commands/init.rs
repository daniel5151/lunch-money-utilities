use anstream::println;
use anyhow::Context;
use anyhow::Result;
use lm_common::init::PlaidAccountChoice;

use crate::cli::InitArgs;
use crate::style::*;

pub async fn run_init(_args: InitArgs, output_path: std::path::PathBuf) -> Result<()> {
    // Load the unified config if it already exists so we upsert the [venmo]
    // section (and the shared [common] key) in place, preserving every other
    // tool's section and all inline comments.
    let mut doc = lm_common::config::editor::read_or_new(&output_path)?;

    println! {};
    println! { "{STYLE_HEADER}⚙️  Configuring Lunch Money Venmo Balance Fixer{STYLE_HEADER:#}" };
    println! { "{STYLE_DIM}─────────────────────────────────────────────────────────────────{STYLE_DIM:#}" };
    println! { "{STYLE_INFO}This wizard will help you set up {}.{STYLE_INFO:#}", output_path.display() };
    println! {};

    let common_cfg = lm_common::config::common_section(&doc)?;
    let api_key = match common_cfg
        .lm_api_key
        .clone()
        .filter(|k| !k.trim().is_empty())
    {
        Some(key) => key,
        None => lm_common::init::prompt_lm_api_key()?,
    };

    println! {};
    println! { "{STYLE_INFO}🔗 Connecting to Lunch Money API to fetch Plaid accounts...{STYLE_INFO:#}" };
    let http_client = reqwest::Client::new();
    let lm_client = lunch_money::client::Client::new(
        http_client,
        api_key.trim().to_string(),
        common_cfg.retry.into(),
    );

    let plaid_accounts = lm_client
        .fetch_plaid_accounts()
        .await
        .context("Failed to fetch Plaid accounts from Lunch Money API")?;

    if plaid_accounts.is_empty() {
        anyhow::bail!("No Plaid accounts found in your Lunch Money account.");
    }

    let choices: Vec<PlaidAccountChoice> =
        plaid_accounts.into_iter().map(PlaidAccountChoice).collect();

    let selected_bank = lm_common::init::select_plaid_account(
        "Select Bank Checking Plaid account:",
        choices.clone(),
    )?;

    let selected_venmo =
        lm_common::init::select_plaid_account("Select Venmo Plaid account:", choices)?;

    // Determine target name for configuration
    let bank_name = selected_bank.config_name();
    let venmo_name = selected_venmo.config_name();

    // Build TOML output
    let toml_content = format!(
        r#"# Lunch Money Venmo Balance Fixer settings
[venmo]
venmo_acct = "{}"
bank_acct = "{}"
"#,
        venmo_name, bank_name
    );

    lm_common::config::editor::upsert_section(&mut doc, "venmo", &toml_content)?;
    lm_common::config::editor::ensure_common_section(&mut doc, api_key.trim());
    lm_common::config::editor::write_secure(&output_path, &doc)?;

    println! {};
    println! { "{STYLE_SUCCESS}🎉 Configuration successfully written to {}{STYLE_SUCCESS:#}", output_path.display() };
    println! { "You can now run: lm-utils venmo-balfixer reconcile 30d" };

    Ok(())
}
