use crate::api::lunch_money::schema::ManualAccountsResponse;
use crate::style::*;
use anstream::println;
use anyhow::Context;
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

pub(crate) async fn run_init() -> anyhow::Result<()> {
    if std::path::Path::new("splitwise-lunchmoney.toml").exists() {
        anyhow::bail!("splitwise-lunchmoney.toml already exists in this directory.");
    }

    println! {};
    println! { "{STYLE_HEADER}⚙️  Configuring Splitwise & Lunch Money Integration{STYLE_HEADER:#}" };
    println! { "{STYLE_DIM}─────────────────────────────────────────────────────────────────{STYLE_DIM:#}" };
    println! { "{STYLE_INFO}This wizard will help you set up splitwise-lunchmoney.toml.{STYLE_INFO:#}" };
    println! {};

    let splitwise_api_key = inquire::Password::new("Splitwise API Key:")
        .with_help_message("Your Splitwise personal API key / Bearer token")
        .with_display_mode(inquire::PasswordDisplayMode::Masked)
        .without_confirmation()
        .prompt()
        .context("Failed to get Splitwise API Key")?;

    let http_client = reqwest::Client::new();

    println! {};
    println! { "{STYLE_INFO}🔗 Connecting to Splitwise API...{STYLE_INFO:#}" };
    let sw_user_response = http_client
        .get("https://secure.splitwise.com/api/v3.0/get_current_user")
        .header("Authorization", format!("Bearer {splitwise_api_key}"))
        .send()
        .await
        .context("Failed to query Splitwise API")?;

    if !sw_user_response.status().is_success() {
        anyhow::bail!("Error querying Splitwise: {}", sw_user_response.status());
    }

    let user_res: CurrentUserResponse = sw_user_response
        .json()
        .await
        .context("Failed to parse Splitwise current user response")?;

    let current_user = user_res.user;
    let selected_user = inquire::Select::new("Select Splitwise User:", vec![current_user])
        .prompt()
        .context("Failed to select Splitwise User")?;

    let splitwise_user_id = selected_user.id;
    let splitwise_user_name = format!(
        "{} {}",
        selected_user.first_name,
        selected_user.last_name.as_deref().unwrap_or("")
    )
    .trim()
    .to_string();

    println! {};
    println! { "  {STYLE_DIM}Fetching Splitwise categories for seeding config...{STYLE_DIM:#}" };
    let sw_client =
        crate::api::splitwise::Client::new(http_client.clone(), splitwise_api_key.clone());
    let sw_categories: crate::api::splitwise::schema::CategoriesResponse = sw_client
        .fetch("get_categories", &[] as &[(&str, &str)])
        .await?;

    let lunch_money_api_key = inquire::Password::new("Lunch Money API Key:")
        .with_help_message("Your Lunch Money developer API key")
        .with_display_mode(inquire::PasswordDisplayMode::Masked)
        .without_confirmation()
        .prompt()
        .context("Failed to get Lunch Money API Key")?;

    println! {};
    println! { "{STYLE_INFO}🔗 Connecting to Lunch Money API...{STYLE_INFO:#}" };
    let lm_client =
        crate::api::lunch_money::Client::new(http_client.clone(), lunch_money_api_key.clone());
    let accounts_res: ManualAccountsResponse = lm_client
        .fetch("manual_accounts", &[] as &[(&str, &str)])
        .await?;

    let inferred = crate::commands::resolve_target_accounts(&accounts_res, &HashMap::new());
    if !inferred.is_empty() {
        println! {};
        println! { "🔍 Discovered the following Splitwise accounts in Lunch Money:" };
        for (curr, id) in &inferred {
            println! { "  • Splitwise {} (ID: {})", curr, id };
        }
    } else {
        println! {};
        println! { "{STYLE_WARNING}⚠️  Warning:{STYLE_WARNING:#} No active manual accounts named 'Splitwise <CURRENCY>' (e.g. 'Splitwise USD') were found in Lunch Money." };
        println! { "Please set up manually managed accounts with these names in your Lunch Money account before syncing." };
    }

    let mut categories_toml = String::new();
    categories_toml.push_str("# \"Payment\" = \"...\"\n");
    for parent in sw_categories.categories {
        for sub in parent.subcategories {
            categories_toml.push_str(&format!("# \"{}:{}\" = \"...\"\n", parent.name, sub.name));
        }
    }
    categories_toml = categories_toml.trim_end().to_string();

    let template = format!(
        r#"[splitwise]
# Your personal Splitwise API key
api_key = "{splitwise_api_key}"

# Your Splitwise user ID
user_id = {splitwise_user_id} # {splitwise_user_name}

# (Optional) Array of Splitwise group IDs or names to ignore
#  HINT: use `splitwise-lunchmoney query splitwise groups` to list your groups and their IDs
# ignored_groups = [123456, "Test Group"]

[lunch_money]
# Your Lunch Money developer API key
api_key = "{lunch_money_api_key}"

# (Optional) Map currencies to custom manual account IDs in Lunch Money
#  For folks who really don't like the `Splitwise - {{currency}}` naming convention
# [lunch_money.custom_accounts]
# USD = 123456
# GBP = 789012

[sync]
# (Optional) Extra tag to associate with transactions where you've loaned out money
#  This can be used to make it easy to spot which splitwise transactions should be
#  (manually) grouped with another account's transaction in lunch money.
#  e.g: grouping a $100 dinner transaction from a credit-card with a $50 splitwise loan
# loan_tag = "Splitwise Loan"

[categories]
# Map Splitwise category names/IDs to Lunch Money category names/IDs (optional)
#  HINT: use `splitwise-lunchmoney query splitwise categories` and
#  `splitwise-lunchmoney query lunchmoney categories` to find names and IDs.
#
{categories_toml}
"#
    );

    fs::write("splitwise-lunchmoney.toml", template)
        .context("Failed to write splitwise-lunchmoney.toml")?;

    println! {};
    println! { "{STYLE_SUCCESS}🎉 Configuration created successfully!{STYLE_SUCCESS:#}" };
    println! { "{STYLE_INFO}Saved to:{STYLE_INFO:#} splitwise-lunchmoney.toml" };
    println! {};
    println! { "{STYLE_DIM}Run {STYLE_DIM:#}{STYLE_HEADER}splitwise-lunchmoney sync window --window \"3 days\"{STYLE_HEADER:#}{STYLE_DIM} to begin syncing.{STYLE_DIM:#}" };
    println! {};
    Ok(())
}
