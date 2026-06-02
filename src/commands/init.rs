use crate::api::lunch_money::schema::ManualAccountsResponse;
use crate::style::*;
use std::collections::HashMap;
use std::fs;

#[derive(serde::Deserialize, Clone)]
struct SplitwiseUser {
    id: u64,
    first_name: String,
    last_name: Option<String>,
}

impl std::fmt::Display for SplitwiseUser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let last = self.last_name.as_deref().unwrap_or("");
        write!(f, "{} {} (ID: {})", self.first_name, last.trim(), self.id)
    }
}

#[derive(serde::Deserialize)]
struct CurrentUserResponse {
    user: SplitwiseUser,
}

pub async fn run_init() {
    if std::path::Path::new("splitwise-lunchmoney.toml").exists() {
        anstream::eprintln! {};
        anstream::eprintln! { "{STYLE_ERROR}❌ Error:{STYLE_ERROR:#} splitwise-lunchmoney.toml already exists in this directory." };
        anstream::eprintln! {};
        std::process::exit(1);
    }

    anstream::println! {};
    anstream::println! { "{STYLE_HEADER}⚙️  Configuring Splitwise & Lunch Money Integration{STYLE_HEADER:#}" };
    anstream::println! { "{STYLE_DIM}─────────────────────────────────────────────────────────────────{STYLE_DIM:#}" };
    anstream::println! { "{STYLE_INFO}This wizard will help you set up splitwise-lunchmoney.toml.{STYLE_INFO:#}" };
    anstream::println! {};

    let splitwise_api_key = inquire::Password::new("Splitwise API Key:")
        .with_help_message("Your Splitwise personal API key / Bearer token")
        .with_display_mode(inquire::PasswordDisplayMode::Masked)
        .without_confirmation()
        .prompt()
        .expect("Failed to get Splitwise API Key");

    let http_client = reqwest::Client::new();

    anstream::println! {};
    anstream::println! { "{STYLE_INFO}🔗 Connecting to Splitwise API...{STYLE_INFO:#}" };
    let sw_user_response = http_client
        .get("https://secure.splitwise.com/api/v3.0/get_current_user")
        .header("Authorization", format!("Bearer {splitwise_api_key}"))
        .send()
        .await
        .expect(
            "Failed to query Splitwise API. Please check your API key and internet connection.",
        );

    if !sw_user_response.status().is_success() {
        anstream::eprintln! {};
        anstream::eprintln! { "{STYLE_ERROR}❌ Error querying Splitwise:{STYLE_ERROR:#} {}", sw_user_response.status() };
        anstream::eprintln! {};
        std::process::exit(1);
    }

    let user_res: CurrentUserResponse = sw_user_response
        .json()
        .await
        .expect("Failed to parse Splitwise current user response");

    let current_user = user_res.user;
    let selected_user = inquire::Select::new("Select Splitwise User:", vec![current_user])
        .prompt()
        .expect("Failed to select Splitwise User");

    let splitwise_user_id = selected_user.id;
    let splitwise_user_name = format!(
        "{} {}",
        selected_user.first_name,
        selected_user.last_name.as_deref().unwrap_or("")
    )
    .trim()
    .to_string();

    let lunch_money_api_key = inquire::Password::new("Lunch Money API Key:")
        .with_help_message("Your Lunch Money developer API key")
        .with_display_mode(inquire::PasswordDisplayMode::Masked)
        .without_confirmation()
        .prompt()
        .expect("Failed to get Lunch Money API Key");

    anstream::println! {};
    anstream::println! { "{STYLE_INFO}🔗 Connecting to Lunch Money API...{STYLE_INFO:#}" };
    let response = http_client
        .get("https://api.lunchmoney.dev/v2/manual_accounts")
        .header("Authorization", format!("Bearer {lunch_money_api_key}"))
        .send()
        .await
        .expect("Failed to query Lunch Money manual accounts. Please check your API key and internet connection.");

    if !response.status().is_success() {
        anstream::eprintln! {};
        anstream::eprintln! { "{STYLE_ERROR}❌ Error querying Lunch Money:{STYLE_ERROR:#} {}", response.status() };
        anstream::eprintln! {};
        std::process::exit(1);
    }

    let accounts_res: ManualAccountsResponse = response
        .json()
        .await
        .expect("Failed to parse Lunch Money manual accounts response");

    let mut supported_currencies = Vec::new();
    anstream::println! {};
    anstream::println! { "{STYLE_INFO}💱 Supported Currencies Setup{STYLE_INFO:#}" };
    anstream::println! { "{STYLE_DIM}Please enter the currencies you want to support (e.g. USD, CAD, GBP).{STYLE_DIM:#}" };
    loop {
        let prompt_msg = if supported_currencies.is_empty() {
            "Enter a currency code you would like to support:"
        } else {
            "Enter another currency code, or press Enter/leave blank to finish:"
        };
        let currency = inquire::Text::new(prompt_msg)
            .prompt()
            .expect("Failed to get currency code");
        let trimmed = currency.trim().to_uppercase();
        if trimmed.is_empty() {
            if supported_currencies.is_empty() {
                anstream::println! { "At least one currency must be specified." };
                continue;
            }
            break;
        }
        if trimmed.len() != 3 || !trimmed.chars().all(|c| c.is_ascii_alphabetic()) {
            anstream::println! { "Please enter a valid 3-letter ISO currency code (e.g. USD)." };
            continue;
        }
        if !supported_currencies.contains(&trimmed) {
            supported_currencies.push(trimmed);
        }
    }

    let mut target_accounts = HashMap::new();
    let mut missing_accounts = Vec::new();

    for currency in &supported_currencies {
        let expected_name = format!("Splitwise {}", currency);
        if let Some(acc) = accounts_res
            .manual_accounts
            .iter()
            .find(|acc| acc.name.eq_ignore_ascii_case(&expected_name))
        {
            target_accounts.insert(currency.clone(), acc.id);
        } else {
            missing_accounts.push(expected_name);
        }
    }

    if !missing_accounts.is_empty() {
        anstream::eprintln! {};
        anstream::eprintln! { "{STYLE_ERROR}❌ Error:{STYLE_ERROR:#} The following required Lunch Money manual accounts are missing:" };
        for acc_name in &missing_accounts {
            anstream::eprintln! { "  • {STYLE_HEADER}{}{STYLE_HEADER:#}", acc_name };
        }
        anstream::eprintln! {};
        anstream::eprintln! { "{STYLE_WARNING}⚠️  Action Required:{STYLE_WARNING:#} Please set up manually managed accounts with these exact names in your Lunch Money account before continuing." };
        anstream::eprintln! {};
        std::process::exit(1);
    }

    let mut target_accounts_toml = String::new();
    for (curr, id) in &target_accounts {
        target_accounts_toml.push_str(&format!("{} = {}\n", curr, id));
    }

    let template = format!(
        r#"[splitwise]
# Your personal Splitwise API key
api_key = "{splitwise_api_key}"

# Your Splitwise user ID
user_id = {splitwise_user_id} # {splitwise_user_name}

# Array of Splitwise group IDs to ignore (optional)
# HINT: use `splitwise-lunchmoney query splitwise get-groups` to easily get IDs
# ignored_groups = [123456, 789012]

[lunch_money]
# Your Lunch Money developer API key
api_key = "{lunch_money_api_key}"

# The mapping from currency code to manual account ID in Lunch Money
[lunch_money.target_accounts]
{target_accounts_toml}"#
    );

    fs::write("splitwise-lunchmoney.toml", template)
        .expect("Failed to write splitwise-lunchmoney.toml");

    anstream::println! {};
    anstream::println! { "{STYLE_SUCCESS}🎉 Configuration created successfully!{STYLE_SUCCESS:#}" };
    anstream::println! { "{STYLE_INFO}Saved to:{STYLE_INFO:#} splitwise-lunchmoney.toml" };
    anstream::println! {};
    anstream::println! { "{STYLE_DIM}Run {STYLE_DIM:#}{STYLE_HEADER}splitwise-lunchmoney sync window --window \"3 days\"{STYLE_HEADER:#}{STYLE_DIM} to begin syncing.{STYLE_DIM:#}" };
    anstream::println! {};
}
