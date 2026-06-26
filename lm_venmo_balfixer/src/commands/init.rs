use crate::cli::InitArgs;
use crate::style::*;
use anstream::println;
use anyhow::Context;
use anyhow::Result;

struct PlaidAccountChoice(lunch_money::plaid_accounts::schemas::PlaidAccount);

impl std::fmt::Display for PlaidAccountChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(ref disp) = self.0.display_name {
            if disp != &self.0.name {
                return write!(f, "{} ({})", disp, self.0.name);
            }
            write!(f, "{}", disp)
        } else {
            write!(f, "{}", self.0.name)
        }
    }
}

impl Clone for PlaidAccountChoice {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

pub async fn run_init(args: InitArgs) -> Result<()> {
    let output_path = args
        .file
        .unwrap_or_else(|| std::path::PathBuf::from(lm_common::config::DEFAULT_CONFIG_FILENAME));

    // Load the unified config if it already exists so we upsert the [venmo]
    // section (and the shared [common] key) in place, preserving every other
    // tool's section and all inline comments.
    let mut doc = lm_common::config::editor::read_or_new(&output_path)?;

    println! {};
    println! { "{STYLE_HEADER}⚙️  Configuring Lunch Money Venmo Balance Fixer{STYLE_HEADER:#}" };
    println! { "{STYLE_DIM}─────────────────────────────────────────────────────────────────{STYLE_DIM:#}" };
    println! { "{STYLE_INFO}This wizard will help you set up {}.{STYLE_INFO:#}", output_path.display() };
    println! {};

    let api_key = inquire::Password::new("Lunch Money API Key:")
        .with_help_message("Your Lunch Money developer API key")
        .with_display_mode(inquire::PasswordDisplayMode::Masked)
        .without_confirmation()
        .prompt()
        .context("Failed to get Lunch Money API Key")?;

    println! {};
    println! { "{STYLE_INFO}🔗 Connecting to Lunch Money API to fetch Plaid accounts...{STYLE_INFO:#}" };
    let http_client = reqwest::Client::new();
    let lm_client = lm_common::lm_client::build(
        http_client,
        api_key.trim().to_string(),
        lm_common::lm_client::RetryConfig::default(),
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

    let selected_bank =
        inquire::Select::new("Select Bank Checking Plaid account:", choices.clone())
            .prompt()
            .context("Failed to select Bank checking account")?;

    let selected_venmo = inquire::Select::new("Select Venmo Plaid account:", choices)
        .prompt()
        .context("Failed to select Venmo account")?;

    // Determine target name for configuration
    let bank_name = selected_bank
        .0
        .display_name
        .clone()
        .unwrap_or(selected_bank.0.name.clone());
    let venmo_name = selected_venmo
        .0
        .display_name
        .clone()
        .unwrap_or(selected_venmo.0.name.clone());

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
