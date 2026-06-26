use crate::AppContext;
use crate::api::lunch_money::TransactionQuery;
use crate::api::lunch_money::schema::UpdateObject;
use crate::api::splitwise::ExpensesQuery;
use crate::style::*;
use anstream::println;
use std::collections::HashMap;
use tabled::Table;
use tabled::Tabled;
use tabled::settings::Style;

#[derive(Tabled)]
struct MigrationRecord {
    #[tabled(rename = "Transaction ID")]
    id: String,
    #[tabled(rename = "Date")]
    date: String,
    #[tabled(rename = "Payee")]
    payee: String,
    #[tabled(rename = "Amount")]
    amount: String,
    #[tabled(rename = "Metadata Size")]
    meta_size: String,
}

pub async fn run_migrate_add_metadata(
    ctx: &AppContext,
    args: crate::cli::MigrateAddMetadataArgs,
) -> anyhow::Result<()> {
    let lm_client = &ctx.lunch_money;
    let sw_client = &ctx.splitwise;

    let dry_run_suffix = if ctx.dry_run {
        format!(" {STYLE_WARNING}[DRY RUN]{STYLE_WARNING:#}")
    } else {
        "".to_string()
    };

    println! {};
    println! { "{STYLE_HEADER}⚡ Retroactive Splitwise Metadata Migration{}{STYLE_HEADER:#}", dry_run_suffix };
    println! { "{STYLE_DIM}──────────────────────────────────────────────────{STYLE_DIM:#}" };

    // Determine scan date range
    let start_date = args.start_date.unwrap_or(jiff::civil::date(2000, 1, 1));
    let end_date = args.end_date.unwrap_or_else(|| {
        jiff::Timestamp::now()
            .to_zoned(jiff::tz::TimeZone::UTC)
            .date()
    });

    println! { "{STYLE_INFO}📅 Date range:{STYLE_INFO:#} {} to {}", start_date, end_date };
    println! {};

    let start_date_str = start_date.to_string();
    let end_date_str = end_date.to_string();

    // Fetch manual accounts and target accounts
    let manual_accounts = lm_client.fetch_manual_accounts().await?;
    let target_accounts = crate::commands::resolve_target_accounts(
        &manual_accounts,
        &ctx.config.custom_accounts,
    );

    if target_accounts.is_empty() {
        println! { "No manual accounts mapped. Nothing to migrate." };
        return Ok(());
    }

    // Fetch Lunch Money transactions
    println! { "  {STYLE_DIM}Fetching Lunch Money transactions...{STYLE_DIM:#}" };
    let mut lm_transactions = Vec::new();
    for &account_id in target_accounts.values() {
        let txs = lm_client
            .fetch_transactions(
                &TransactionQuery::builder()
                    .start_date(start_date_str.clone())
                    .end_date(end_date_str.clone())
                    .manual_account_id(account_id)
                    .limit(1000)
                    .include_group_children(true)
                    .include_split_parents(true)
                    .include_metadata(true)
                    .build(),
            )
            .await?;
        lm_transactions.extend(txs);
    }

    // Filter transactions that have Splitwise external ID but are missing or have malformed metadata
    let mut target_txs = Vec::new();
    for t in lm_transactions {
        if let Some(crate::api::ExternalId::Splitwise(sw_id)) = t.external_id {
            let is_valid = matches!(
                t.custom_metadata,
                Some(crate::api::lunch_money::schema::MaybeLunchMoneyTxMetadata::Expected(_))
            );
            if !is_valid {
                target_txs.push((t, sw_id));
            }
        }
    }

    if target_txs.is_empty() {
        println! { "{STYLE_SUCCESS}✨ No Splitwise transactions with missing or malformed metadata found in this range.{STYLE_SUCCESS:#}" };
        return Ok(());
    }

    println! { "  {STYLE_INFO}Found {} Splitwise transactions with missing or malformed metadata.{STYLE_INFO:#}", target_txs.len() };
    println! { "  {STYLE_DIM}Fetching matching Splitwise expenses...{STYLE_DIM:#}" };

    // Fetch Splitwise expenses in the range to build a cache
    let sw_expenses = sw_client
        .fetch_expenses(&ExpensesQuery {
            dated_after: Some(start_date_str.clone()),
            dated_before: Some(format!("{}T23:59:59Z", end_date_str)),
            limit: Some(0),
            ..Default::default()
        })
        .await?;

    let mut sw_expense_cache: HashMap<u64, crate::api::splitwise::Expense> =
        sw_expenses.into_iter().map(|e| (e.parsed.id, e)).collect();

    let mut updates = Vec::new();
    let mut skipped = 0;

    for (t, sw_id) in target_txs {
        let expense = match sw_expense_cache.get(&sw_id) {
            Some(e) => Some(e.clone()),
            None => {
                // Try fetching individually from Splitwise
                match sw_client.fetch_expense(sw_id).await {
                    Ok(e) => {
                        sw_expense_cache.insert(sw_id, e.clone());
                        Some(e)
                    }
                    Err(err) => {
                        println! { "  ⚠️  {STYLE_WARNING}Warning:{STYLE_WARNING:#} Could not fetch Splitwise expense ID {} for Lunch Money transaction ID {} ('{}'): {}", sw_id, t.id, t.payee, err };
                        skipped += 1;
                        None
                    }
                }
            }
        };

        if let Some(exp) = expense {
            let desired_metadata = crate::api::lunch_money::schema::LunchMoneyTxMetadata::Import {
                delta_transaction_ids: Vec::new(),
                original: exp.parsed.clone().into(),
            };

            updates.push(
                UpdateObject::builder()
                    .id(t.id)
                    .date(t.date)
                    .amount(t.amount)
                    .currency(t.currency)
                    .payee(t.payee)
                    .notes(t.notes.unwrap_or_default())
                    .custom_metadata(desired_metadata)
                    .build(),
            );
        }
    }

    if updates.is_empty() {
        println! { "  No metadata updates could be prepared (skipped all {} transactions).", skipped };
        return Ok(());
    }

    println! {};
    println! { "  {STYLE_INFO}Prepared {} metadata updates.{STYLE_INFO:#}", updates.len() };
    if skipped > 0 {
        println! { "  ⚠️  {STYLE_WARNING}Skipped {} transactions due to Splitwise fetch errors.{STYLE_WARNING:#}", skipped };
    }

    updates.sort_by_key(|u| std::cmp::Reverse(u.date));

    let super::MaxWidths {
        max_num_len,
        max_currency_len,
    } = super::compute_max_widths(updates.iter().map(|u| (u.amount, &u.currency)));

    let mut records = Vec::new();
    for u in &updates {
        let meta_size = u
            .custom_metadata
            .as_ref()
            .and_then(|m| serde_json::to_string(m).ok())
            .map(|s| s.len())
            .unwrap_or(0);

        let size_str = if meta_size > 4096 {
            format! { "{STYLE_ERROR}{} bytes (EXCEEDS LIMIT){STYLE_ERROR:#}", meta_size }
        } else if meta_size > 3000 {
            format! { "{STYLE_WARNING}{} bytes (WARNING: close to limit){STYLE_WARNING:#}", meta_size }
        } else {
            format! { "{STYLE_DIM}{} bytes{STYLE_DIM:#}", meta_size }
        };

        let amount_colored = super::format_colored_balance(
            u.amount,
            &u.currency,
            max_num_len,
            max_currency_len,
            false,
        );

        records.push(MigrationRecord {
            id: u.id.to_string(),
            date: u.date.to_string(),
            payee: u.payee.clone(),
            amount: amount_colored,
            meta_size: size_str,
        });
    }

    println! {};
    let mut table = Table::new(records);
    table.with(Style::rounded());
    println! { "{}", table };

    if ctx.dry_run {
        println! {};
        println! { "  {STYLE_WARNING}[Dry Run] No changes were written to Lunch Money.{STYLE_WARNING:#}" };
    } else {
        println! {};
        println! { "  {STYLE_DIM}Applying updates in Lunch Money...{STYLE_DIM:#}" };
        for chunk in updates.chunks(500) {
            lm_client.update_transactions(chunk).await?;
        }
        println! {};
        println! { "{STYLE_SUCCESS}✨ Metadata migration complete! Successfully updated {} transactions.{STYLE_SUCCESS:#}", updates.len() };
    }

    Ok(())
}
