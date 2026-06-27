use std::collections::HashSet;

use anstream::println;
use anyhow::Context;
use anyhow::Result;
use lunch_money::client::Client as LunchMoneyClient;
use lunch_money::core::CategoryId;
use lunch_money::core::PlaidAccountId;
use lunch_money::transactions::query_params::TransactionQuery;
use lunch_money::transactions::schemas::InsertObject;
use lunch_money::transactions::schemas::Transaction;
use lunch_money::transactions::schemas::TransactionStatus;
use rust_decimal::Decimal;

use crate::cli::ReconcileArgs;
use crate::config::Config;
use crate::style::*;

pub async fn run_reconcile(
    cx: &lm_common::tool::ToolContext,
    config: &Config,
    api_key: &str,
    retry: lm_common::config::RetryConfig,
    args: ReconcileArgs,
) -> Result<()> {
    if api_key.trim().is_empty() {
        anyhow::bail!("lm_api_key is empty in the configuration file");
    }

    // Calculate dates
    let end_date = jiff::Zoned::now().date();
    let start_date = end_date
        .checked_sub(args.duration)
        .context("Failed to calculate start date")?;

    println! { "{STYLE_HEADER}Lunch Money Venmo Balance Fixer (lm-venmo-balfixer){STYLE_HEADER:#}" };
    println! { "{STYLE_INFO}Scanning from {} to {}{STYLE_INFO:#}", start_date, end_date };

    let lm_client = cx.lunch_money(api_key.to_string(), retry.into());

    // 2. Fetch Plaid Accounts and resolve Bank and Venmo IDs by display name / name
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

    let bank_id = resolve_account_id(&plaid_accounts, &config.bank_acct).ok_or_else(|| {
        anyhow::anyhow!(
            "Could not resolve Bank account with name '{}' from Plaid accounts.",
            config.bank_acct
        )
    })?;

    println! { "  Bank Checking Account ID: {} (resolved from '{}')", bank_id, config.bank_acct };
    println! { "  Venmo Account ID:         {} (resolved from '{}')", venmo_id, config.venmo_acct };

    // 3. Fetch categories and resolve Transfer category ID
    let cat_query = lunch_money::categories::query_params::CategoryQuery::builder()
        .format("flattened".to_string())
        .build();
    let categories = lm_client
        .fetch_categories(&cat_query)
        .await
        .context("Failed to fetch categories")?;

    let transfer_category_id = find_transfer_category(&categories);
    if let Some(cat_id) = transfer_category_id {
        println! { "  Resolved 'Transfer' category ID: {}", cat_id };
    } else {
        println! { "{STYLE_WARNING}⚠️  Warning: Could not resolve 'Transfer' category. Synthetic transactions will be uncategorized.{STYLE_WARNING:#}" };
    }

    // 4. Fetch transaction history
    println! { "Fetching transactions..." };
    let bank_txs = fetch_all_transactions(&lm_client, bank_id, start_date, end_date)
        .await
        .context("Failed to fetch bank transactions")?;
    let venmo_txs = fetch_all_transactions(&lm_client, venmo_id, start_date, end_date)
        .await
        .context("Failed to fetch Venmo transactions")?;

    println! { "Fetched {} bank checking transactions and {} Venmo transactions.", bank_txs.len(), venmo_txs.len() };

    // 5. Filter to candidate transfer events
    let bank_transfers: Vec<Transaction<serde_json::Value, String>> = bank_txs
        .into_iter()
        .filter(|t| is_bank_transfer(t, bank_id))
        .collect();
    let venmo_transfers: Vec<Transaction<serde_json::Value, String>> = venmo_txs
        .into_iter()
        .filter(|t| is_venmo_transfer(t, venmo_id))
        .collect();

    println! { "Found {} candidate bank checking transfers and {} candidate Venmo transfers.", bank_transfers.len(), venmo_transfers.len() };

    // 6. Match existing transfers to avoid duplicates
    let mut matched_bank_ids = HashSet::new();

    for vt in &venmo_transfers {
        if vt.amount >= Decimal::ZERO {
            continue;
        }
        let v_amt_abs = vt.amount.abs();

        for bt in &bank_transfers {
            if bt.amount <= Decimal::ZERO {
                continue;
            }
            if matched_bank_ids.contains(&bt.id) {
                continue;
            }

            let b_amt_abs = bt.amount.abs();
            if v_amt_abs == b_amt_abs {
                // Check if dates are within 5 days
                let days_diff = vt
                    .date
                    .until(bt.date)
                    .map(|s| s.get_days().abs())
                    .unwrap_or(999);
                if days_diff <= 5 {
                    matched_bank_ids.insert(bt.id);
                    println! { "{STYLE_DIM}Matched transfer: Bank ID {} ({} on {}) with Venmo ID {} ({} on {}){STYLE_DIM:#}",
                    bt.id, bt.amount, bt.date, vt.id, vt.amount, vt.date };
                    break;
                }
            }
        }
    }

    println! { "Matched {} transfers, leaving {} bank transfers unmatched.", matched_bank_ids.len(), bank_transfers.len() - matched_bank_ids.len() };

    // 7. Generate synthetic funding events for remaining unmatched checking transfers
    let mut synthetic_txs = Vec::new();

    for bt in &bank_transfers {
        if bt.amount <= Decimal::ZERO {
            continue;
        }
        if matched_bank_ids.contains(&bt.id) {
            continue;
        }

        println! { "Generating synthetic Venmo transfer for bank transaction: Date: {}, Amount: {}, ID: {}", bt.date, bt.amount, bt.id };

        let insert_obj = InsertObject::<serde_json::Value, String>::builder()
            .date(bt.date)
            .amount(-bt.amount) // Invert amount to represent an inflow to Venmo
            .payee("Venmo Transfer (Synthetic)".to_string())
            .notes(format!("(Bank Date: {}, ID: {})", bt.date, bt.id))
            .maybe_category_id(transfer_category_id)
            .plaid_account_id(venmo_id)
            .external_id(format!("synthetic-venmo-{}", bt.id))
            // Leave synthetics unreviewed so they surface in the review queue
            // for the user to eyeball, rather than landing pre-cleared.
            .status(TransactionStatus::Unreviewed)
            .build();

        synthetic_txs.push(insert_obj);
    }

    if synthetic_txs.is_empty() {
        println! { "{STYLE_SUCCESS}No missing funding events found. Venmo and bank checking are fully reconciled.{STYLE_SUCCESS:#}" };
    } else if cx.dry_run {
        println! { "[Dry Run] Would insert {} synthetic transactions:", synthetic_txs.len() };
        for tx in &synthetic_txs {
            let notes_str = tx.notes.as_deref().unwrap_or("");
            let payee_str = tx.payee.as_deref().unwrap_or("");
            println! { "{STYLE_WARNING}Would create synthetic transaction: {} on {} for {} {}{STYLE_WARNING:#}",
            payee_str, tx.date, tx.amount, notes_str };
        }
    } else {
        println! { "Inserting {} synthetic transactions...", synthetic_txs.len() };
        let inserted_resp = lm_client
            .insert_transactions::<serde_json::Value, String, serde_json::Value, String>(
                &synthetic_txs,
            )
            .await
            .context("Failed to insert synthetic transactions")?;

        for tx in &inserted_resp.transactions {
            let notes_str = tx.notes.as_deref().unwrap_or("");
            println! { "{STYLE_SUCCESS}Successfully created synthetic transaction: {} on {} for {} {}{STYLE_SUCCESS:#}",
            tx.payee, tx.date, tx.amount, notes_str };
        }
        for s in &inserted_resp.skipped_duplicates {
            println! { "{STYLE_WARNING}Skipped duplicate transaction: index {}, reason: {}{STYLE_WARNING:#}",
            s.request_transactions_index, s.reason };
        }
    }

    Ok(())
}

fn resolve_account_id(
    plaid_accounts: &[lunch_money::plaid_accounts::schemas::PlaidAccount],
    target_name: &str,
) -> Option<PlaidAccountId> {
    // 1. Try to match display_name (case-insensitive)
    for acc in plaid_accounts {
        if let Some(ref disp) = acc.display_name {
            if disp.eq_ignore_ascii_case(target_name) {
                return Some(acc.id);
            }
        }
    }
    // 2. Try to match name (case-insensitive)
    for acc in plaid_accounts {
        if acc.name.eq_ignore_ascii_case(target_name) {
            return Some(acc.id);
        }
    }
    None
}

fn find_transfer_category(
    categories: &[lunch_money::categories::schemas::Category],
) -> Option<CategoryId> {
    // 1. Search for exact match "Payment, Transfer"
    for cat in categories {
        if !cat.is_group && cat.name.eq_ignore_ascii_case("Payment, Transfer") {
            return Some(cat.id);
        }
    }
    // 2. Search for exact match "Transfer"
    for cat in categories {
        if !cat.is_group && cat.name.eq_ignore_ascii_case("Transfer") {
            return Some(cat.id);
        }
    }
    // 3. Search for name containing "Transfer"
    for cat in categories {
        if !cat.is_group && cat.name.to_lowercase().contains("transfer") {
            return Some(cat.id);
        }
    }
    None
}

async fn fetch_all_transactions(
    client: &LunchMoneyClient,
    account_id: PlaidAccountId,
    start_date: jiff::civil::Date,
    end_date: jiff::civil::Date,
) -> Result<Vec<Transaction<serde_json::Value, String>>> {
    let mut transactions = Vec::new();
    let mut offset = 0;
    let limit = 1000;

    loop {
        let query = TransactionQuery::builder()
            .start_date(start_date.to_string())
            .end_date(end_date.to_string())
            // NOTE: include_group_children surfaces children of grouped/split
            // transactions. A grouped/split bank transfer could therefore appear
            // as both a parent and its children, double-counting candidates. This
            // is very unlikely for Venmo ACH funding rows, so it's left unguarded;
            // add a split/group-parent filter here if it ever surfaces.
            .include_group_children(true)
            .plaid_account_id(account_id)
            .limit(limit)
            .offset(offset)
            .build();

        let response = client
            .fetch_transactions::<serde_json::Value, String>(&query)
            .await?;
        let len = response.transactions.len();
        transactions.extend(response.transactions);

        if response.has_more && len > 0 {
            offset += len as u32;
        } else {
            break;
        }
    }

    Ok(transactions)
}

fn is_bank_transfer(t: &Transaction<serde_json::Value, String>, bank_id: PlaidAccountId) -> bool {
    let matches_account = t.plaid_account_id == Some(bank_id);
    if !matches_account {
        return false;
    }
    // Skip pending transactions: a pending transfer may later re-post with a
    // changed amount/date or disappear entirely. Synthesizing against one risks
    // orphaning the synthetic, so only reconcile settled transactions.
    if t.is_pending {
        return false;
    }
    if t.amount <= Decimal::ZERO {
        return false;
    }
    let payee_match = t.payee.to_lowercase().contains("venmo");
    let orig_match = t
        .original_name
        .as_ref()
        .map(|n| n.to_lowercase().contains("venmo"))
        .unwrap_or(false);
    let notes_match = t
        .notes
        .as_ref()
        .map(|n| n.to_lowercase().contains("venmo"))
        .unwrap_or(false);
    payee_match || orig_match || notes_match
}

fn is_venmo_transfer(t: &Transaction<serde_json::Value, String>, venmo_id: PlaidAccountId) -> bool {
    let matches_account = t.plaid_account_id == Some(venmo_id);
    if !matches_account {
        return false;
    }
    // Skip pending transactions for symmetry with the bank side: a pending
    // Venmo inflow could otherwise spuriously "match" a bank transfer and
    // suppress a synthetic that should be created once it settles.
    if t.is_pending {
        return false;
    }
    let payee_match = t.payee.to_lowercase().contains("transfer");
    let orig_match = t
        .original_name
        .as_ref()
        .map(|n| n.to_lowercase().contains("transfer"))
        .unwrap_or(false);
    let notes_match = t
        .notes
        .as_ref()
        .map(|n| n.to_lowercase().contains("transfer"))
        .unwrap_or(false);
    payee_match || orig_match || notes_match
}
