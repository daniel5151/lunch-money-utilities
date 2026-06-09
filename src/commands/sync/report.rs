use crate::style::*;
use anstream::println;
use anyhow::Context;
use rust_decimal::Decimal;
use std::collections::HashMap;
use tabled::Table;
use tabled::Tabled;
use tabled::settings::Style;

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

    let amount_style = if amount.is_sign_negative() {
        STYLE_ERROR
    } else {
        STYLE_SUCCESS
    };
    let amount_plain = crate::commands::format_aligned_balance(
        amount,
        currency,
        max_num_len,
        max_currency_len,
        false,
    );
    let amount_colored = format!("{}{}{:#}", amount_style, amount_plain, amount_style);

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

pub struct PrintAndLogSyncPlanArgs<'a> {
    pub plan: &'a super::SyncPlan,
    pub dry_run: bool,
    pub lm_category_names: &'a HashMap<u64, String>,
    pub sw_expense_categories: &'a HashMap<crate::api::ExternalId, Option<(u32, String)>>,
    pub sw_category_id_to_path: &'a HashMap<u32, String>,
    pub lm_tx_categories: &'a HashMap<u64, (Option<crate::api::ExternalId>, Option<u64>)>,
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
            lunch_money_id: Option<u64>,
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
    if !plan.deletes.is_empty() {
        println! { "🗑️  {STYLE_WARNING}Deleting {STYLE_WARNING:#}{} old/modified transaction(s) from Lunch Money:", plan.deletes.len() };
        let crate::commands::MaxWidths {
            max_num_len,
            max_currency_len,
        } = crate::commands::compute_max_widths(
            plan.deletes.iter().map(|t| (t.amount, &t.currency)),
        );
        let mut records = Vec::new();
        for t in &plan.deletes {
            let category_name = t
                .category_id
                .and_then(|id| lm_category_names.get(&id).cloned());
            let sw_category_name = t
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
                payee: &t.payee,
                amount: t.amount,
                currency: &t.currency,
                date: t.date,
                notes: t.notes.as_deref().unwrap_or(""),
                sw_category_name,
                lm_category_name: category_name.as_deref(),
                max_num_len,
                max_currency_len,
            }));
        }
        let mut table = Table::new(records);
        table.with(Style::rounded());
        println! { "{}" , table };
        println! {};
    }

    if !plan.updates.is_empty() {
        println! { "✎  {STYLE_INFO}Updating {STYLE_INFO:#}{} modified transaction(s) in Lunch Money:", plan.updates.len() };
        let crate::commands::MaxWidths {
            max_num_len,
            max_currency_len,
        } = crate::commands::compute_max_widths(
            plan.updates.iter().map(|u| (u.amount, &u.currency)),
        );
        let mut records = Vec::new();
        for u in &plan.updates {
            let (external_id, category_id) = lm_tx_categories
                .get(&u.id)
                .map(|(ext_id, cat_id)| (ext_id.as_ref(), *cat_id))
                .unwrap_or((None, None));
            let sw_category_name = external_id
                .and_then(|ext_id| sw_expense_categories.get(ext_id))
                .and_then(|cat_info| {
                    cat_info.as_ref().and_then(|(cat_id, cat_name)| {
                        sw_category_id_to_path
                            .get(cat_id)
                            .map(|s| s.as_str())
                            .or(Some(cat_name.as_str()))
                    })
                });
            let category_name = category_id.and_then(|id| lm_category_names.get(&id).cloned());
            records.push(to_sync_record(ToSyncRecordArgs {
                payee: &u.payee,
                amount: u.amount,
                currency: &u.currency,
                date: u.date,
                notes: &u.notes,
                sw_category_name,
                lm_category_name: category_name.as_deref(),
                max_num_len,
                max_currency_len,
            }));
        }
        let mut table = Table::new(records);
        table.with(Style::rounded());
        println! { "{}" , table };
        println! {};
    }

    if !plan.inserts.is_empty() {
        println! { "✓  {STYLE_SUCCESS}Inserting {STYLE_SUCCESS:#}{} new transaction(s) to Lunch Money:", plan.inserts.len() };
        let crate::commands::MaxWidths {
            max_num_len,
            max_currency_len,
        } = crate::commands::compute_max_widths(
            plan.inserts.iter().map(|ins| (ins.amount, &ins.currency)),
        );
        let mut records = Vec::new();
        for ins in &plan.inserts {
            let category_name = ins
                .category_id
                .and_then(|id| lm_category_names.get(&id).cloned());
            let sw_category_name =
                sw_expense_categories
                    .get(&ins.external_id)
                    .and_then(|cat_info| {
                        cat_info.as_ref().and_then(|(cat_id, cat_name)| {
                            sw_category_id_to_path
                                .get(cat_id)
                                .map(|s| s.as_str())
                                .or(Some(cat_name.as_str()))
                        })
                    });
            records.push(to_sync_record(ToSyncRecordArgs {
                payee: &ins.payee,
                amount: ins.amount,
                currency: &ins.currency,
                date: ins.date,
                notes: &ins.notes,
                sw_category_name,
                lm_category_name: category_name.as_deref(),
                max_num_len,
                max_currency_len,
            }));
        }
        let mut table = Table::new(records);
        table.with(Style::rounded());
        println! { "{}" , table };
        println! {};
    }

    if plan.deletes.is_empty() && plan.updates.is_empty() && plan.inserts.is_empty() {
        println! { "{STYLE_SUCCESS}✨ No changes detected. Lunch Money manual account is up-to-date!{STYLE_SUCCESS:#}" };
    } else if dry_run {
        println! { "{STYLE_WARNING}⚠️ Dry run complete! No changes were made to Lunch Money.{STYLE_WARNING:#}" };
    }
    println! {};
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_execute_sync_actions_csv() {
        use crate::api::Currency;
        use crate::api::ExternalId;
        use crate::api::lunch_money::schema::InsertObject;
        use crate::api::lunch_money::schema::Transaction;
        use crate::api::lunch_money::schema::TransactionStatus;
        use crate::api::lunch_money::schema::UpdateObject;
        use rust_decimal::Decimal;
        use std::collections::HashMap;

        let temp_dir = std::env::temp_dir();
        let csv_path = temp_dir.join("sync_actions_test.csv");
        if csv_path.exists() {
            let _ = std::fs::remove_file(&csv_path);
        }

        let deletes = vec![Transaction {
            id: 10,
            date: jiff::civil::date(2026, 6, 1),
            amount: Decimal::new(-1000, 2),
            currency: Currency::new("USD"),
            payee: "Delete Payee".to_string(),
            notes: Some("Delete Notes".to_string()),
            external_id: Some(ExternalId::Splitwise(100)),
            manual_account_id: Some(999),
            is_split_parent: None,
            group_parent_id: None,
            status: TransactionStatus::Reviewed,
            category_id: Some(5),
            custom_metadata: None,
        }];

        let updates = vec![UpdateObject {
            id: 20,
            date: jiff::civil::date(2026, 6, 2),
            amount: Decimal::new(2000, 2),
            currency: Currency::new("USD"),
            payee: "Update Payee".to_string(),
            notes: "Update Notes".to_string(),
            custom_metadata: None,
        }];

        let inserts = vec![InsertObject {
            date: jiff::civil::date(2026, 6, 3),
            amount: Decimal::new(3000, 2),
            currency: Currency::new("USD"),
            payee: "Insert Payee".to_string(),
            notes: "Insert Notes".to_string(),
            external_id: ExternalId::Splitwise(300),
            manual_account_id: 999,
            status: TransactionStatus::Unreviewed,
            tag_ids: None,
            category_id: Some(6),
            custom_metadata: None,
        }];

        let mut lm_category_names = HashMap::new();
        lm_category_names.insert(5, "Delete Category".to_string());
        lm_category_names.insert(6, "Insert Category".to_string());
        lm_category_names.insert(7, "Update Category".to_string());

        let mut sw_expense_categories = HashMap::new();
        sw_expense_categories.insert(
            ExternalId::Splitwise(100),
            Some((100, "SW Cat".to_string())),
        );

        let sw_category_id_to_path = HashMap::new();

        let mut lm_tx_categories = HashMap::new();
        lm_tx_categories.insert(20, (Some(ExternalId::Splitwise(200)), Some(7)));

        let plan = super::super::SyncPlan {
            inserts,
            updates,
            deletes,
            tags_to_create: vec![],
        };

        print_and_log_sync_plan(PrintAndLogSyncPlanArgs {
            plan: &plan,
            dry_run: true,
            lm_category_names: &lm_category_names,
            sw_expense_categories: &sw_expense_categories,
            sw_category_id_to_path: &sw_category_id_to_path,
            lm_tx_categories: &lm_tx_categories,
            csv_path: Some(&csv_path),
        })
        .unwrap();

        assert!(csv_path.exists());
        let content = std::fs::read_to_string(&csv_path).unwrap();
        let lines: Vec<&str> = content.lines().collect();

        assert_eq!(lines.len(), 4);
        assert_eq!(
            lines[0],
            "operation,lunch_money_id,external_id,date,payee,amount,currency,notes,category"
        );
        assert_eq!(
            lines[1],
            "delete,10,splitwise_100,2026-06-01,Delete Payee,-10.00,USD,Delete Notes,Delete Category"
        );
        assert_eq!(
            lines[2],
            "update,20,splitwise_200,2026-06-02,Update Payee,20.00,USD,Update Notes,Update Category"
        );
        assert_eq!(
            lines[3],
            "insert,,splitwise_300,2026-06-03,Insert Payee,30.00,USD,Insert Notes,Insert Category"
        );

        let _ = std::fs::remove_file(csv_path);
    }
}
