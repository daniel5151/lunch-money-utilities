use anyhow::Context;
use anyhow::Result;
use anstream::println;
use lunch_money::transactions::schemas::UpdateObject;

use crate::cli::PayeeArgs;
use crate::config::Config;
use crate::commands::reconcile::{fetch_all_transactions, resolve_account_id};
use crate::style::*;

pub async fn run_payee(
    cx: &lm_common::tool::ToolContext,
    config: &Config,
    api_key: &str,
    retry: lm_common::config::RetryConfig,
    args: PayeeArgs,
) -> Result<()> {
    if api_key.trim().is_empty() {
        anyhow::bail!("lm_api_key is empty in the configuration file");
    }

    // Calculate dates
    let end_date = jiff::Zoned::now().date();
    let start_date = end_date
        .checked_sub(args.duration)
        .context("Failed to calculate start date")?;

    println! { "{STYLE_HEADER}Lunch Money Venmo Payee Fixer (lm-venmo-plaidfix){STYLE_HEADER:#}" };
    println! { "{STYLE_INFO}Scanning Venmo transactions from {} to {}{STYLE_INFO:#}", start_date, end_date };

    let lm_client = cx.lunch_money(api_key.to_string(), retry.into());

    // Fetch Plaid Accounts and resolve Venmo ID
    println! { "Fetching Plaid accounts..." };
    let plaid_accounts = lm_client
        .fetch_plaid_accounts()
        .await
        .context("Failed to fetch Plaid accounts")?;

    let venmo_id = resolve_account_id(&plaid_accounts, &config.venmo_acct).ok_or_else(|| {
        anyhow::anyhow!(
            "Could not resolve Venmo account with name '{}' from Plaid accounts.",
            config.venmo_acct
        )
    })?;

    println! { "  Venmo Account ID: {} (resolved from '{}')", venmo_id, config.venmo_acct };

    // Fetch Venmo transactions
    println! { "Fetching Venmo transactions..." };
    let venmo_txs = fetch_all_transactions(&lm_client, venmo_id, start_date, end_date)
        .await
        .context("Failed to fetch Venmo transactions")?;

    println! { "Fetched {} Venmo transactions.", venmo_txs.len() };

    println! { "Checking for Venmo transactions requiring name/note split..." };
    let mut name_updates = Vec::new();
    for tx in &venmo_txs {
        // Skip transactions modified by a human unless force_fixup is enabled
        if tx.created_at != tx.updated_at && !args.force_fixup {
            continue;
        }

        if let Some(ref orig_name) = tx.original_name {
            if let Some((clean_name, venmo_note)) = parse_venmo_original_name(orig_name) {
                let current_notes = tx.notes.as_deref().unwrap_or("").trim();
                let target_notes = if current_notes.is_empty() {
                    venmo_note.clone()
                } else {
                    let expected_suffix = format!("({})", venmo_note);
                    if current_notes == venmo_note || current_notes.contains(&expected_suffix) {
                        current_notes.to_string()
                    } else {
                        format!("{} ({})", current_notes, venmo_note)
                    }
                };

                if tx.payee != clean_name || tx.notes.as_deref().unwrap_or("") != target_notes {
                    let update_obj = UpdateObject::<serde_json::Value, String>::builder()
                        .id(tx.id)
                        .payee(clean_name.clone())
                        .notes(target_notes.clone())
                        .build();
                    name_updates.push((tx.id, clean_name, target_notes, update_obj));
                }
            }
        }
    }

    if name_updates.is_empty() {
        println! { "{STYLE_SUCCESS}No transactions require name/note fixups.{STYLE_SUCCESS:#}" };
    } else if cx.dry_run {
        println! { "[Dry Run] Would fix up {} transaction names/notes:", name_updates.len() };
        for (id, clean_name, venmo_note, _) in &name_updates {
            println! { "{STYLE_WARNING}Would update transaction ID {}: set payee to '{}', set notes to '{}'{STYLE_WARNING:#}",
            id, clean_name, venmo_note };
        }
    } else {
        println! { "Updating payee/notes for {} transactions...", name_updates.len() };
        let update_payload: Vec<UpdateObject<serde_json::Value, String>> = name_updates
            .iter()
            .map(|(_, _, _, obj)| obj.clone())
            .collect();

        lm_client
            .update_transactions::<serde_json::Value, String>(&update_payload)
            .await
            .context("Failed to update transaction payees/notes")?;

        for (id, clean_name, venmo_note, _) in &name_updates {
            println! { "{STYLE_SUCCESS}Successfully fixed up transaction ID {}: payee='{}', notes='{}'{STYLE_SUCCESS:#}",
            id, clean_name, venmo_note };
        }
    }

    Ok(())
}

fn parse_venmo_original_name(orig_name: &str) -> Option<(String, String)> {
    let first_quote = orig_name.find('"')?;
    let last_quote = orig_name.rfind('"')?;
    if first_quote < last_quote {
        let name = orig_name[..first_quote].trim().to_string();
        let note = orig_name[first_quote + 1..last_quote].to_string();
        if !name.is_empty() {
            return Some((name, note));
        }
    }
    None
}
