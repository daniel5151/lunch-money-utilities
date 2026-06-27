use crate::style::*;
use anstream::println;
use anyhow::Context;
use std::collections::HashMap;

struct SplitwiseUser(crate::api::splitwise::schema::User);

impl std::fmt::Display for SplitwiseUser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let last = self.0.last_name.as_deref().unwrap_or("");
        write!(
            f,
            "{} {} (ID: {})",
            self.0.first_name,
            last.trim(),
            self.0.id
        )
    }
}

pub(crate) async fn run_init(args: crate::cli::InitArgs) -> anyhow::Result<()> {
    let output_path = args
        .file
        .unwrap_or_else(|| std::path::PathBuf::from(lm_common::config::DEFAULT_CONFIG_FILENAME));

    // Load the unified config if it already exists so we upsert the [splitwise]
    // section (and the shared [common] key) in place, preserving every other
    // tool's section and all inline comments.
    let mut doc = lm_common::config::editor::read_or_new(&output_path)?;

    println! {};
    println! { "{STYLE_HEADER}⚙️  Configuring Splitwise & Lunch Money Integration{STYLE_HEADER:#}" };
    println! { "{STYLE_DIM}─────────────────────────────────────────────────────────────────{STYLE_DIM:#}" };
    println! { "{STYLE_INFO}This wizard will help you set up {}.{STYLE_INFO:#}", output_path.display() };
    println! {};

    let splitwise_api_key = inquire::Password::new("Splitwise API Key:")
        .with_help_message("Your Splitwise personal API key / Bearer token")
        .with_display_mode(inquire::PasswordDisplayMode::Masked)
        .without_confirmation()
        .prompt()
        .context("Failed to get Splitwise API Key")?;

    let http_client = reqwest::Client::new();
    let sw_client =
        crate::api::splitwise::Client::new(http_client.clone(), splitwise_api_key.clone());

    println! {};
    println! { "{STYLE_INFO}🔗 Connecting to Splitwise API...{STYLE_INFO:#}" };
    let current_user = sw_client
        .fetch_current_user()
        .await
        .context("Failed to query Splitwise API")?;

    let selected_user =
        inquire::Select::new("Select Splitwise User:", vec![SplitwiseUser(current_user)])
            .prompt()
            .context("Failed to select Splitwise User")?;

    let splitwise_user_id = selected_user.0.id;
    let splitwise_user_name = format!(
        "{} {}",
        selected_user.0.first_name,
        selected_user.0.last_name.as_deref().unwrap_or("")
    )
    .trim()
    .to_string();

    println! {};
    println! { "  {STYLE_DIM}Fetching Splitwise categories for seeding config...{STYLE_DIM:#}" };
    let sw_categories = sw_client.fetch_categories().await?;

    let lunch_money_api_key = match lm_common::config::common_section(&doc)
        .ok()
        .and_then(|c| c.lm_api_key)
        .filter(|k| !k.trim().is_empty())
    {
        Some(key) => key,
        None => lm_common::init::prompt_lm_api_key()?,
    };

    println! {};
    println! { "{STYLE_INFO}🔗 Connecting to Lunch Money API...{STYLE_INFO:#}" };
    let lm_client =
        crate::api::lunch_money::Client::new(http_client.clone(), lunch_money_api_key.clone());
    let manual_accounts = lm_client.fetch_manual_accounts().await?;

    let inferred = crate::commands::resolve_target_accounts(&manual_accounts, &HashMap::new());
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

    println! {};

    let backdated_tag = inquire::Text::new("Backdated Tag:")
        .with_default("🧾🕰️ Splitwise Backdated")
        .with_help_message("Tag applied to newly imported transactions whose original Splitwise date falls outside the sync window")
        .prompt()
        .context("Failed to get backdated_tag")?;

    let updated_tag = inquire::Text::new("Updated Tag:")
        .with_default("🧾⏫ Splitwise Updated")
        .with_help_message("Tag applied to the original older Lunch Money transaction when its Splitwise expense is updated or deleted")
        .prompt()
        .context("Failed to get updated_tag")?;

    let orphaned_tag = inquire::Text::new("Orphaned Tag:")
        .with_default("🧾⚠️ Splitwise Orphaned")
        .with_help_message(
            "Tag applied to orphaned delta transactions when the original transaction is deleted",
        )
        .prompt()
        .context("Failed to get orphaned_tag")?;

    let use_loan_tag = inquire::Confirm::new("(Optional) Would you like to set a \"loan tag\"?")
        .with_default(false)
        .with_help_message("This tag will be auto-applied to imported transactions where you've loaned money to others, and can make it easier to spot what Splitwise transactions might need to be (manually) grouped with another account's transaction in Lunch Money (e.g. grouping a $100 dinner transaction from a credit card with a $50 Splitwise loan). NOTE: this can be set up using a lunch-money rule, but can be done via splitwise-sync as a convenience.")
        .prompt()
        .context("Failed to get loan_tag preference")?;

    let loan_tag = if use_loan_tag {
        let tag = inquire::Text::new("Loan Tag:")
            .with_default("💵 Splitwise")
            .with_help_message("Tag applied to transactions where you've loaned out money")
            .prompt()
            .context("Failed to get loan_tag value")?;
        Some(tag)
    } else {
        None
    };

    let loan_tag_line = match &loan_tag {
        Some(tag) => format!(r#"loan_tag = "{}""#, tag),
        None => {
            r#"# (Optional) Extra tag to associate with transactions where you've loaned out money
#  This can be used to make it easy to spot which splitwise transactions should be
#  (manually) grouped with another account's transaction in lunch money.
#  e.g: grouping a $100 dinner transaction from a credit-card with a $50 splitwise loan
# loan_tag = "💵 Splitwise""#
                .to_string()
        }
    };

    let mut categories_toml = String::new();
    categories_toml.push_str("# \"Payment\" = \"...\"\n");
    for parent in sw_categories {
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
#  HINT: use `lm-utils splitwise-sync query splitwise groups` to list your groups and their IDs
# ignored_groups = [123456, "Test Group"]

# (Optional) Map currencies to custom manual account IDs in Lunch Money
#  For folks who really don't like the `Splitwise - {{currency}}` naming convention
# [splitwise.custom_accounts]
# USD = 123456
# GBP = 789012

[splitwise.sync]
backdated_tag = "{backdated_tag}"
updated_tag = "{updated_tag}"
orphaned_tag = "{orphaned_tag}"
{loan_tag_line}

[splitwise.categories]
# Map Splitwise category names/IDs to Lunch Money category names/IDs (optional)
#  HINT: use `lm-utils splitwise-sync query splitwise categories` and
#  `lm-utils splitwise-sync query lunchmoney categories` to find names and IDs.
#
{categories_toml}
"#
    );

    lm_common::config::editor::upsert_section(&mut doc, "splitwise", &template)?;
    lm_common::config::editor::ensure_common_section(&mut doc, lunch_money_api_key.trim());
    lm_common::config::editor::write_secure(&output_path, &doc)?;

    println! {};
    println! { "{STYLE_SUCCESS}🎉 Configuration created successfully!{STYLE_SUCCESS:#}" };
    println! { "{STYLE_INFO}Saved to:{STYLE_INFO:#} {}", output_path.display() };
    println! {};
    println! { "{STYLE_DIM}Run {STYLE_DIM:#}{STYLE_HEADER}lm-utils splitwise-sync sync window --window \"3 days\"{STYLE_HEADER:#}{STYLE_DIM} to begin syncing.{STYLE_DIM:#}" };
    println! {};
    Ok(())
}
