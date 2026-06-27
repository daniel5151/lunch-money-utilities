use std::collections::HashMap;

use anstream::println;
use anyhow::Context;
use rust_decimal::Decimal;
use tabled::Table;
use tabled::Tabled;
use tabled::settings::Style;

use crate::api::lunch_money::schema::CategoryId;
use crate::api::lunch_money::schema::TransactionId;
use crate::style::*;

#[derive(Tabled)]
struct SyncRecord {
    #[tabled(rename = "Date")]
    date: String,
    #[tabled(rename = "Payee")]
    payee: String,
    #[tabled(rename = "Category (Splitwise)")]
    sw_category: String,
    #[tabled(rename = "Category (Lunch Money)")]
    lm_category: String,
    #[tabled(rename = "Amount")]
    amount: String,
    #[tabled(rename = "Notes")]
    notes: String,
}

struct ToSyncRecordArgs<'a> {
    payee: &'a str,
    amount: Decimal,
    currency: &'a crate::api::Currency,
    date: jiff::civil::Date,
    notes: &'a str,
    sw_category_name: Option<&'a str>,
    lm_category_name: Option<&'a str>,
    max_num_len: usize,
    max_currency_len: usize,
}

/// Formats a transaction sync record into a `SyncRecord`.
/// We accept the pre-calculated `max_num_len` and `max_currency_len` to format the transaction
/// amount cell with alignment, ensuring decimals and currency codes line up vertically.
fn to_sync_record(args: ToSyncRecordArgs<'_>) -> SyncRecord {
    let ToSyncRecordArgs {
        payee,
        amount,
        currency,
        date,
        notes,
        sw_category_name,
        lm_category_name,
        max_num_len,
        max_currency_len,
    } = args;
    let date_str = date.strftime("%Y-%m-%d").to_string();

    let mut clean_payee = payee.to_string();
    if clean_payee.starts_with("Splitwise - ") {
        clean_payee = clean_payee["Splitwise - ".len()..].to_string();
    }
    if clean_payee.chars().count() > 50 {
        clean_payee = clean_payee.chars().take(47).collect::<String>();
        clean_payee.push_str("...");
    }

    let sw_clean = match sw_category_name {
        Some("Uncategorized:General") => "",
        Some(other) => other,
        None => "",
    };

    let sw_is_uncategorized = matches!(sw_category_name, None | Some("Uncategorized:General"));
    let lm_clean = if sw_is_uncategorized {
        lm_category_name.unwrap_or("")
    } else {
        lm_category_name.unwrap_or("?")
    };

    let amount_colored = crate::commands::format_colored_balance(
        amount,
        currency,
        max_num_len,
        max_currency_len,
        false,
    );

    let notes_colored = if notes.trim().is_empty() {
        "".to_string()
    } else {
        format!("{}{}{:#}", STYLE_DIM, notes.trim(), STYLE_DIM)
    };

    SyncRecord {
        date: date_str,
        payee: clean_payee,
        sw_category: sw_clean.to_string(),
        lm_category: lm_clean.to_string(),
        amount: amount_colored,
        notes: notes_colored,
    }
}

struct PrintItem<'a> {
    payee: &'a str,
    amount: Decimal,
    currency: &'a crate::api::Currency,
    date: jiff::civil::Date,
    notes: &'a str,
    category_id: Option<CategoryId>,
    external_id: Option<crate::api::ExternalId>,
}

fn print_transaction_table(
    title: &str,
    items: &[PrintItem<'_>],
    lm_category_names: &HashMap<CategoryId, String>,
    sw_expense_categories: &HashMap<crate::api::ExternalId, Option<(u32, String)>>,
    sw_category_id_to_path: &HashMap<u32, String>,
) {
    if items.is_empty() {
        return;
    }

    let crate::commands::MaxWidths {
        max_num_len,
        max_currency_len,
    } = crate::commands::compute_max_widths(items.iter().map(|item| (item.amount, item.currency)));

    let mut sorted_items: Vec<&PrintItem<'_>> = items.iter().collect();
    sorted_items.sort_by_key(|item| std::cmp::Reverse(item.date));

    let mut records = Vec::new();
    for item in sorted_items {
        let lm_cat_name = item
            .category_id
            .and_then(|id| lm_category_names.get(&id).cloned());
        let sw_category_name = item
            .external_id
            .as_ref()
            .and_then(|ext_id| sw_expense_categories.get(ext_id))
            .and_then(|cat_info| {
                cat_info.as_ref().and_then(|(cat_id, cat_name)| {
                    sw_category_id_to_path
                        .get(cat_id)
                        .map(|s| s.as_str())
                        .or(Some(cat_name.as_str()))
                })
            });

        records.push(to_sync_record(ToSyncRecordArgs {
            payee: item.payee,
            amount: item.amount,
            currency: item.currency,
            date: item.date,
            notes: item.notes,
            sw_category_name,
            lm_category_name: lm_cat_name.as_deref(),
            max_num_len,
            max_currency_len,
        }));
    }

    println!("{}", title);
    let mut table = Table::new(records);
    table.with(Style::rounded());
    println!("{}", table);
    println!();
}

pub struct PrintAndLogSyncPlanArgs<'a> {
    pub plan: &'a super::SyncPlan,
    pub dry_run: bool,
    pub lm_category_names: &'a HashMap<CategoryId, String>,
    pub sw_expense_categories: &'a HashMap<crate::api::ExternalId, Option<(u32, String)>>,
    pub sw_category_id_to_path: &'a HashMap<u32, String>,
    pub lm_tx_categories:
        &'a HashMap<TransactionId, (Option<crate::api::ExternalId>, Option<CategoryId>)>,
    pub csv_path: Option<&'a std::path::Path>,
}

pub fn print_and_log_sync_plan(args: PrintAndLogSyncPlanArgs<'_>) -> anyhow::Result<()> {
    let PrintAndLogSyncPlanArgs {
        plan,
        dry_run,
        lm_category_names,
        sw_expense_categories,
        sw_category_id_to_path,
        lm_tx_categories,
        csv_path,
    } = args;

    if let Some(path) = csv_path {
        #[derive(serde::Serialize)]
        struct CsvRow<'a> {
            operation: &'static str,
            lunch_money_id: Option<TransactionId>,
            external_id: Option<String>,
            date: String,
            payee: &'a str,
            amount: Decimal,
            currency: &'a str,
            notes: &'a str,
            category: &'a str,
        }

        let mut wtr = csv::Writer::from_path(path)
            .with_context(|| format!("Failed to create CSV file at '{}'", path.display()))?;

        // Write deletes
        for t in &plan.deletes {
            let category_name = t
                .category_id
                .and_then(|id| lm_category_names.get(&id).cloned())
                .unwrap_or_default();
            let ext_id_str = t.external_id.as_ref().map(|ext_id| ext_id.to_string());
            wtr.serialize(CsvRow {
                operation: "delete",
                lunch_money_id: Some(t.id),
                external_id: ext_id_str,
                date: t.date.to_string(),
                payee: &t.payee,
                amount: t.amount,
                currency: t.currency.as_str(),
                notes: t.notes.as_deref().unwrap_or(""),
                category: &category_name,
            })
            .context("Failed to write CSV row")?;
        }

        // Write updates
        for u in &plan.updates {
            let (external_id, category_id) = lm_tx_categories
                .get(&u.id)
                .map(|(ext_id, cat_id)| (ext_id.as_ref(), *cat_id))
                .unwrap_or((None, None));
            let category_name = category_id
                .and_then(|id| lm_category_names.get(&id).cloned())
                .unwrap_or_default();
            let ext_id_str = external_id.map(|ext_id| ext_id.to_string());
            wtr.serialize(CsvRow {
                operation: "update",
                lunch_money_id: Some(u.id),
                external_id: ext_id_str,
                date: u.date.to_string(),
                payee: &u.payee,
                amount: u.amount,
                currency: u.currency.as_str(),
                notes: &u.notes,
                category: &category_name,
            })
            .context("Failed to write CSV row")?;
        }

        // Write inserts
        for ins in &plan.inserts {
            let category_name = ins
                .category_id
                .and_then(|id| lm_category_names.get(&id).cloned())
                .unwrap_or_default();
            wtr.serialize(CsvRow {
                operation: "insert",
                lunch_money_id: None,
                external_id: Some(ins.external_id.to_string()),
                date: ins.date.to_string(),
                payee: &ins.payee,
                amount: ins.amount,
                currency: ins.currency.as_str(),
                notes: &ins.notes,
                category: &category_name,
            })
            .context("Failed to write CSV row")?;
        }

        wtr.flush().context("Failed to flush CSV file")?;
    }

    if dry_run {
        for tag_name in &plan.tags_to_create {
            println! { "   {STYLE_WARNING}Would create tag:{STYLE_WARNING:#} '{}'", tag_name };
        }
    }

    // Execute batches output
    let delete_items: Vec<_> = plan
        .deletes
        .iter()
        .map(|t| PrintItem {
            payee: &t.payee,
            amount: t.amount,
            currency: &t.currency,
            date: t.date,
            notes: t.notes.as_deref().unwrap_or(""),
            category_id: t.category_id,
            external_id: t.external_id.clone(),
        })
        .collect();
    print_transaction_table(
        &format!(
            "🗑️  {STYLE_WARNING}Deleting {STYLE_WARNING:#}{} old/modified transaction(s) from Lunch Money:",
            plan.deletes.len()
        ),
        &delete_items,
        lm_category_names,
        sw_expense_categories,
        sw_category_id_to_path,
    );

    let update_items: Vec<_> = plan
        .updates
        .iter()
        .map(|u| {
            let (external_id, category_id) = lm_tx_categories
                .get(&u.id)
                .map(|(ext_id, cat_id)| (ext_id.clone(), *cat_id))
                .unwrap_or((None, None));
            PrintItem {
                payee: &u.payee,
                amount: u.amount,
                currency: &u.currency,
                date: u.date,
                notes: &u.notes,
                category_id,
                external_id,
            }
        })
        .collect();
    print_transaction_table(
        &format!(
            "✎  {STYLE_INFO}Updating {STYLE_INFO:#}{} modified transaction(s) in Lunch Money:",
            plan.updates.len()
        ),
        &update_items,
        lm_category_names,
        sw_expense_categories,
        sw_category_id_to_path,
    );

    let insert_items: Vec<_> = plan
        .inserts
        .iter()
        .map(|ins| PrintItem {
            payee: &ins.payee,
            amount: ins.amount,
            currency: &ins.currency,
            date: ins.date,
            notes: &ins.notes,
            category_id: ins.category_id,
            external_id: Some(ins.external_id.clone()),
        })
        .collect();
    print_transaction_table(
        &format!(
            "✓  {STYLE_SUCCESS}Inserting {STYLE_SUCCESS:#}{} new transaction(s) to Lunch Money:",
            plan.inserts.len()
        ),
        &insert_items,
        lm_category_names,
        sw_expense_categories,
        sw_category_id_to_path,
    );

    if plan.deletes.is_empty() && plan.updates.is_empty() && plan.inserts.is_empty() {
        println! { "{STYLE_SUCCESS}✨ No changes detected. Lunch Money manual account is up-to-date!{STYLE_SUCCESS:#}" };
    } else if dry_run {
        println! { "{STYLE_WARNING}⚠️ Dry run complete! No changes were made to Lunch Money.{STYLE_WARNING:#}" };
    }
    println! {};
    Ok(())
}
