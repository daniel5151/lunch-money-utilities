use crate::api::lunch_money::schema::AccountType;
use crate::api::lunch_money::schema::CategoryId;
use crate::api::lunch_money::schema::ManualAccount;
use crate::api::lunch_money::schema::ManualAccountId;
use crate::api::lunch_money::schema::TagId;
use crate::api::lunch_money::schema::TransactionId;
use crate::style::*;
use anstream::println;
use rust_decimal::Decimal;
use std::collections::HashMap;
use tabled::Table;
use tabled::Tabled;
use tabled::settings::Style;

#[derive(Tabled)]
struct RecoveryRecord {
    #[tabled(rename = "Action")]
    action: String,
    #[tabled(rename = "Date")]
    date: String,
    #[tabled(rename = "Payee")]
    payee: String,
    #[tabled(rename = "Amount")]
    amount: String,
    #[tabled(rename = "Notes")]
    notes: String,
}

pub struct ApplySyncPlanArgs<'a> {
    pub plan: &'a mut super::SyncPlan,
    pub lm_client: &'a crate::api::lunch_money::Client,
    pub manual_accounts: &'a [ManualAccount],
    pub target_accounts: &'a HashMap<crate::api::Currency, ManualAccountId>,
    pub tag_id: Option<TagId>,
    pub loan_tag_id: Option<TagId>,
    pub updated_tag_id: Option<TagId>,
    pub lm_transactions: &'a [crate::api::lunch_money::schema::Transaction],
    pub expenses: &'a [crate::api::splitwise::Expense],
    pub config: &'a crate::config::Config,
    pub backdated_tag_id: Option<TagId>,
    pub sync_window_start: Option<jiff::civil::Date>,
    pub no_ignore: bool,
    pub lm_category_names: &'a HashMap<CategoryId, String>,
    pub csv_path: Option<&'a std::path::Path>,
}

pub async fn apply_sync_plan(args: ApplySyncPlanArgs<'_>) -> anyhow::Result<()> {
    let ApplySyncPlanArgs {
        plan,
        lm_client,
        manual_accounts,
        target_accounts,
        tag_id,
        loan_tag_id,
        updated_tag_id,
        lm_transactions,
        expenses,
        config,
        backdated_tag_id,
        sync_window_start,
        no_ignore,
        lm_category_names,
        csv_path,
    } = args;

    let mut recovered_transactions = HashMap::new();

    if !plan.deletes.is_empty() {
        let delete_ids: Vec<TransactionId> = plan.deletes.iter().map(|t| t.id).collect();
        lm_client.delete_transactions(&delete_ids).await?;
    }

    if !plan.updates.is_empty() {
        for chunk in plan.updates.chunks(500) {
            let mut chunk_txs = chunk.to_vec();
            for u in &mut chunk_txs {
                let is_loan = manual_accounts
                    .iter()
                    .find(|acc| target_accounts.get(&u.currency).copied() == Some(acc.id))
                    .map(|acc| acc.account_type == AccountType::Loan)
                    .unwrap_or(false);
                if is_loan {
                    u.amount = -u.amount;
                }
            }
            lm_client.update_transactions(&chunk_txs).await?;
        }
    }

    let mut inserted_deltas = HashMap::new();

    if !plan.inserts.is_empty() {
        let mut delta_inserts = Vec::new();

        for chunk in plan.inserts.chunks(500) {
            let mut chunk_txs = chunk.to_vec();
            for ins in &mut chunk_txs {
                let is_loan = manual_accounts
                    .iter()
                    .find(|acc| acc.id == ins.manual_account_id)
                    .map(|acc| acc.account_type == AccountType::Loan)
                    .unwrap_or(false);
                if is_loan {
                    ins.amount = -ins.amount;
                }
            }
            let response = lm_client.insert_transactions(&chunk_txs).await?;
            for inserted_tx in response.transactions {
                if let Some(crate::api::lunch_money::schema::MaybeLunchMoneyTxMetadata::Expected(
                    crate::api::lunch_money::schema::LunchMoneyTxMetadata::Delta {
                        original_transaction_id,
                        ..
                    },
                )) = &inserted_tx.custom_metadata
                {
                    delta_inserts.push((*original_transaction_id, inserted_tx.id));
                    let mut tx = inserted_tx.clone();
                    let is_loan = tx
                        .manual_account_id
                        .and_then(|acc_id| manual_accounts.iter().find(|acc| acc.id == acc_id))
                        .map(|acc| acc.account_type == AccountType::Loan)
                        .unwrap_or(false);
                    if is_loan {
                        tx.amount = -tx.amount;
                    }
                    inserted_deltas.insert(inserted_tx.id, tx);
                }
            }

            if !response.skipped_duplicates.is_empty() {
                println! {
                    "🔄  {STYLE_INFO}Recovering {} time-shifted transaction(s) outside the window via delta fixup:{STYLE_INFO:#}",
                    response.skipped_duplicates.len()
                };

                let mut extra_updates = Vec::new();
                let mut extra_inserts = Vec::new();

                for dup in &response.skipped_duplicates {
                    let skipped_ins = &chunk_txs[dup.request_transactions_index];
                    if let crate::api::ExternalId::Splitwise(splitwise_id) = skipped_ins.external_id
                    {
                        if let Some(expense) = expenses.iter().find(|e| e.parsed.id == splitwise_id)
                        {
                            println! {
                                "   • Splitwise ID {}: Existing transaction ID {} found outside window. Calculating delta...",
                                splitwise_id,
                                dup.existing_transaction_id
                            };

                            let existing_lm_opt = lm_client
                                .fetch_transaction_by_id(dup.existing_transaction_id)
                                .await?;
                            let mut existing_lm = match existing_lm_opt {
                                Some(tx) => tx,
                                None => {
                                    println!(
                                        "  {STYLE_WARNING}Warning: Matched duplicate transaction ID {} was deleted on Lunch Money. Skipping recovery.{STYLE_WARNING:#}",
                                        dup.existing_transaction_id
                                    );
                                    continue;
                                }
                            };
                            let is_loan = existing_lm
                                .manual_account_id
                                .and_then(|acc_id| {
                                    manual_accounts.iter().find(|acc| acc.id == acc_id)
                                })
                                .map(|acc| acc.account_type == AccountType::Loan)
                                .unwrap_or(false);
                            if is_loan {
                                existing_lm.amount = -existing_lm.amount;
                            }

                            recovered_transactions.insert(existing_lm.id, existing_lm.clone());

                            let mut delta_txs = Vec::new();
                            let mut active_delta_ids = Vec::new();
                            let mut delta_ids_modified = false;
                            if let Some(crate::api::lunch_money::schema::MaybeLunchMoneyTxMetadata::Expected(
                                crate::api::lunch_money::schema::LunchMoneyTxMetadata::Import {
                                    delta_transaction_ids,
                                    ..
                                },
                            )) = &existing_lm.custom_metadata
                            {
                                for &d_id in delta_transaction_ids {
                                    match lm_client.fetch_transaction_by_id(d_id).await? {
                                        Some(mut d_tx) => {
                                            if is_loan {
                                                d_tx.amount = -d_tx.amount;
                                            }
                                            delta_txs.push(d_tx);
                                            active_delta_ids.push(d_id);
                                        }
                                        None => {
                                            println!("  {STYLE_WARNING}Warning: Referenced delta transaction ID {} was deleted on Lunch Money. Removing reference.{STYLE_WARNING:#}", d_id);
                                            delta_ids_modified = true;
                                        }
                                    }
                                }
                            }

                            if delta_ids_modified {
                                if let Some(crate::api::lunch_money::schema::MaybeLunchMoneyTxMetadata::Expected(
                                    crate::api::lunch_money::schema::LunchMoneyTxMetadata::Import {
                                        delta_transaction_ids,
                                        ..
                                    },
                                )) = &mut existing_lm.custom_metadata {
                                    *delta_transaction_ids = active_delta_ids;
                                }
                            }

                            let mut lm_by_id = HashMap::new();
                            lm_by_id.insert(existing_lm.id, existing_lm.clone());
                            for d_tx in &delta_txs {
                                lm_by_id.insert(d_tx.id, d_tx.clone());
                            }

                            let net_balance = expense
                                .parsed
                                .users
                                .iter()
                                .find(|u| u.user_id == config.splitwise.user_id)
                                .map(|u| u.net_balance)
                                .unwrap_or(Decimal::ZERO);

                            let is_ignored = !no_ignore
                                && expense.parsed.group_id.is_some_and(|gid| {
                                    config.splitwise.is_group_ignored(gid, None)
                                });

                            let target_amount = if expense.parsed.deleted_at.is_some()
                                || is_ignored
                                || net_balance.is_zero()
                            {
                                Decimal::ZERO
                            } else {
                                net_balance
                            };

                            let date_civil =
                                expense.parsed.date.to_zoned(jiff::tz::TimeZone::UTC).date();

                            let mut dup_updates = Vec::new();
                            let mut dup_inserts = Vec::new();

                            let currency_changed =
                                existing_lm.currency != expense.parsed.currency_code;
                            if currency_changed {
                                println! {
                                    "   • Splitwise ID {}: Currency changed from {} to {}. Zeroing out old transaction and inserting new one.",
                                    splitwise_id,
                                    existing_lm.currency,
                                    expense.parsed.currency_code
                                };

                                super::diff::apply_lpp_delta_engine(
                                    &existing_lm,
                                    Decimal::ZERO,
                                    expense,
                                    &lm_by_id,
                                    &mut dup_updates,
                                    &mut dup_inserts,
                                    config,
                                    target_accounts,
                                    backdated_tag_id,
                                    updated_tag_id,
                                    date_civil,
                                    &skipped_ins.payee,
                                    sync_window_start,
                                )?;

                                let mut found = false;
                                for u in &mut dup_updates {
                                    if u.id == existing_lm.id {
                                        u.external_id = Some(None);
                                        found = true;
                                        break;
                                    }
                                }
                                if !found {
                                    dup_updates.push(crate::api::lunch_money::schema::UpdateObject {
                                        id: existing_lm.id,
                                        date: existing_lm.date,
                                        amount: existing_lm.amount,
                                        currency: existing_lm.currency.clone(),
                                        payee: existing_lm.payee.clone(),
                                        notes: existing_lm.notes.clone().unwrap_or_default(),
                                        custom_metadata: existing_lm.custom_metadata.clone().and_then(|m| match m {
                                            crate::api::lunch_money::schema::MaybeLunchMoneyTxMetadata::Expected(metadata) => Some(metadata),
                                            _ => None,
                                        }),
                                        additional_tag_ids: None,
                                        external_id: Some(None),
                                    });
                                }

                                if !target_amount.is_zero() {
                                    let mut new_ins = skipped_ins.clone();
                                    new_ins.amount = target_amount;
                                    new_ins.date = jiff::Timestamp::now()
                                        .to_zoned(jiff::tz::TimeZone::UTC)
                                        .date();
                                    new_ins.notes = format!(
                                        "(Original Date: {}) {}",
                                        date_civil, expense.parsed.description
                                    );

                                    let mut tag_ids = new_ins.tag_ids.unwrap_or_default();
                                    if let Some(bt_id) = backdated_tag_id {
                                        if !tag_ids.contains(&bt_id) {
                                            tag_ids.push(bt_id);
                                        }
                                    }
                                    if let Some(tid) = tag_id {
                                        if !tag_ids.contains(&tid) {
                                            tag_ids.push(tid);
                                        }
                                    }
                                    if target_amount > Decimal::ZERO {
                                        if let Some(ltid) = loan_tag_id {
                                            if !tag_ids.contains(&ltid) {
                                                tag_ids.push(ltid);
                                            }
                                        }
                                    }
                                    new_ins.tag_ids = if tag_ids.is_empty() {
                                        None
                                    } else {
                                        Some(tag_ids)
                                    };

                                    dup_inserts.push(new_ins);
                                }
                            } else {
                                super::diff::apply_lpp_delta_engine(
                                    &existing_lm,
                                    target_amount,
                                    expense,
                                    &lm_by_id,
                                    &mut dup_updates,
                                    &mut dup_inserts,
                                    config,
                                    target_accounts,
                                    backdated_tag_id,
                                    updated_tag_id,
                                    date_civil,
                                    &skipped_ins.payee,
                                    sync_window_start,
                                )?;
                            }

                            extra_updates.extend(dup_updates);
                            extra_inserts.extend(dup_inserts);
                        }
                    }
                }

                if !extra_updates.is_empty() {
                    for chunk in extra_updates.chunks(500) {
                        let mut chunk_txs = chunk.to_vec();
                        for u in &mut chunk_txs {
                            let is_loan = manual_accounts
                                .iter()
                                .find(|acc| {
                                    target_accounts.get(&u.currency).copied() == Some(acc.id)
                                })
                                .map(|acc| acc.account_type == AccountType::Loan)
                                .unwrap_or(false);
                            if is_loan {
                                u.amount = -u.amount;
                            }
                        }
                        lm_client.update_transactions(&chunk_txs).await?;
                    }
                }

                if !extra_inserts.is_empty() {
                    for chunk in extra_inserts.chunks(500) {
                        let mut chunk_txs = chunk.to_vec();
                        for ins in &mut chunk_txs {
                            let is_loan = manual_accounts
                                .iter()
                                .find(|acc| acc.id == ins.manual_account_id)
                                .map(|acc| acc.account_type == AccountType::Loan)
                                .unwrap_or(false);
                            if is_loan {
                                ins.amount = -ins.amount;
                            }
                        }
                        let response = lm_client.insert_transactions(&chunk_txs).await?;
                        for inserted_tx in response.transactions {
                            if let Some(crate::api::lunch_money::schema::MaybeLunchMoneyTxMetadata::Expected(
                                crate::api::lunch_money::schema::LunchMoneyTxMetadata::Delta {
                                    original_transaction_id,
                                    ..
                                },
                            )) = &inserted_tx.custom_metadata
                            {
                                delta_inserts.push((*original_transaction_id, inserted_tx.id));
                                let mut tx = inserted_tx.clone();
                                let is_loan = tx
                                    .manual_account_id
                                    .and_then(|acc_id| {
                                        manual_accounts.iter().find(|acc| acc.id == acc_id)
                                    })
                                    .map(|acc| acc.account_type == AccountType::Loan)
                                    .unwrap_or(false);
                                if is_loan {
                                    tx.amount = -tx.amount;
                                }
                                inserted_deltas.insert(inserted_tx.id, tx);
                            }
                        }
                    }
                }

                let mut recovery_records = Vec::new();
                let all_amounts_and_currencies: Vec<_> = extra_updates
                    .iter()
                    .map(|u| (u.amount, &u.currency))
                    .chain(extra_inserts.iter().map(|ins| (ins.amount, &ins.currency)))
                    .collect();

                if !all_amounts_and_currencies.is_empty() {
                    let crate::commands::MaxWidths {
                        max_num_len,
                        max_currency_len,
                    } = crate::commands::compute_max_widths(all_amounts_and_currencies);

                    for u in &extra_updates {
                        let amount_colored = crate::commands::format_colored_balance(
                            u.amount,
                            &u.currency,
                            max_num_len,
                            max_currency_len,
                            false,
                        );

                        recovery_records.push(RecoveryRecord {
                            action: format!("{}✎ Update (Parent){}", STYLE_INFO, STYLE_INFO),
                            date: u.date.to_string(),
                            payee: u.payee.clone(),
                            amount: amount_colored,
                            notes: format!("{}{}{:#}", STYLE_DIM, u.notes.trim(), STYLE_DIM),
                        });
                    }

                    for ins in &extra_inserts {
                        let amount_colored = crate::commands::format_colored_balance(
                            ins.amount,
                            &ins.currency,
                            max_num_len,
                            max_currency_len,
                            false,
                        );

                        recovery_records.push(RecoveryRecord {
                            action: format!("{}✚ Insert (Delta){}", STYLE_SUCCESS, STYLE_SUCCESS),
                            date: ins.date.to_string(),
                            payee: ins.payee.clone(),
                            amount: amount_colored,
                            notes: format!("{}{}{:#}", STYLE_DIM, ins.notes.trim(), STYLE_DIM),
                        });
                    }

                    if !recovery_records.is_empty() {
                        println! {};
                        println! { "🔧  {STYLE_SUCCESS}Applying recovery actions for time-shifted transaction(s):{STYLE_SUCCESS:#}" };
                        let mut table = Table::new(recovery_records);
                        table.with(Style::rounded());
                        println! { "{}" , table };
                        println! {};
                    }
                }

                if let Some(path) = csv_path {
                    #[derive(serde::Serialize)]
                    struct RecoveryCsvRow<'a> {
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

                    if let Ok(file) = std::fs::OpenOptions::new().append(true).open(path) {
                        let mut wtr = csv::Writer::from_writer(file);

                        for u in &extra_updates {
                            let category_name = lm_transactions
                                .iter()
                                .find(|t| t.id == u.id)
                                .or_else(|| recovered_transactions.get(&u.id))
                                .and_then(|t| t.category_id)
                                .and_then(|id| lm_category_names.get(&id).cloned())
                                .unwrap_or_default();

                            let ext_id_str = lm_transactions
                                .iter()
                                .find(|t| t.id == u.id)
                                .or_else(|| recovered_transactions.get(&u.id))
                                .and_then(|t| t.external_id.as_ref().map(|ext| ext.to_string()));

                            let row = RecoveryCsvRow {
                                operation: "update",
                                lunch_money_id: Some(u.id),
                                external_id: ext_id_str,
                                date: u.date.to_string(),
                                payee: &u.payee,
                                amount: u.amount,
                                currency: u.currency.as_str(),
                                notes: &u.notes,
                                category: &category_name,
                            };
                            let _ = wtr.serialize(row);
                        }

                        for ins in &extra_inserts {
                            let category_name = ins
                                .category_id
                                .and_then(|id| lm_category_names.get(&id).cloned())
                                .unwrap_or_default();

                            let row = RecoveryCsvRow {
                                operation: "insert",
                                lunch_money_id: None,
                                external_id: Some(ins.external_id.to_string()),
                                date: ins.date.to_string(),
                                payee: &ins.payee,
                                amount: ins.amount,
                                currency: ins.currency.as_str(),
                                notes: &ins.notes,
                                category: &category_name,
                            };
                            let _ = wtr.serialize(row);
                        }
                        let _ = wtr.flush();
                    }
                }
            }
        }

        if !delta_inserts.is_empty() {
            let mut linkage_updates = Vec::new();
            let mut deltas_by_orig: HashMap<TransactionId, Vec<TransactionId>> = HashMap::new();
            for (orig_id, delta_id) in delta_inserts {
                deltas_by_orig.entry(orig_id).or_default().push(delta_id);
            }

            for (orig_id, new_delta_ids) in deltas_by_orig {
                if let Some(orig_tx) = lm_transactions
                    .iter()
                    .find(|t| t.id == orig_id)
                    .or_else(|| recovered_transactions.get(&orig_id))
                {
                    let mut updated_delta_ids = if let Some(
                        crate::api::lunch_money::schema::MaybeLunchMoneyTxMetadata::Expected(
                            crate::api::lunch_money::schema::LunchMoneyTxMetadata::Import {
                                delta_transaction_ids,
                                ..
                            },
                        ),
                    ) = &orig_tx.custom_metadata
                    {
                        delta_transaction_ids.clone()
                    } else {
                        Vec::new()
                    };

                    updated_delta_ids.extend(new_delta_ids);

                    let original_expense = if let Some(
                        crate::api::lunch_money::schema::MaybeLunchMoneyTxMetadata::Expected(
                            crate::api::lunch_money::schema::LunchMoneyTxMetadata::Import {
                                original,
                                ..
                            },
                        ),
                    ) = &orig_tx.custom_metadata
                    {
                        original.clone()
                    } else {
                        continue;
                    };

                    let desired_metadata =
                        crate::api::lunch_money::schema::LunchMoneyTxMetadata::Import {
                            delta_transaction_ids: updated_delta_ids.clone(),
                            original: original_expense.clone(),
                        };

                    let splitwise_id = original_expense.id;

                    for &d_id in &updated_delta_ids {
                        if let Some(d_tx) = lm_transactions
                            .iter()
                            .find(|t| t.id == d_id)
                            .or_else(|| recovered_transactions.get(&d_id))
                            .or_else(|| inserted_deltas.get(&d_id))
                        {
                            linkage_updates.push(crate::api::lunch_money::schema::UpdateObject {
                                id: d_tx.id,
                                date: d_tx.date,
                                amount: d_tx.amount,
                                currency: d_tx.currency.clone(),
                                payee: d_tx.payee.clone(),
                                notes: d_tx.notes.clone().unwrap_or_default(),
                                custom_metadata: Some(
                                    crate::api::lunch_money::schema::LunchMoneyTxMetadata::Delta {
                                        original_transaction_id: orig_tx.id,
                                        delta_transaction_ids: updated_delta_ids.clone(),
                                        splitwise_id,
                                    },
                                ),
                                additional_tag_ids: None,
                                external_id: None,
                            });
                        }
                    }

                    let mut tag_ids = Vec::new();
                    if let Some(ut_id) = updated_tag_id {
                        tag_ids.push(ut_id);
                    }

                    linkage_updates.push(crate::api::lunch_money::schema::UpdateObject {
                        id: orig_tx.id,
                        date: orig_tx.date,
                        amount: orig_tx.amount,
                        currency: orig_tx.currency.clone(),
                        payee: orig_tx.payee.clone(),
                        notes: orig_tx.notes.clone().unwrap_or_default(),
                        custom_metadata: Some(desired_metadata),
                        additional_tag_ids: if tag_ids.is_empty() {
                            None
                        } else {
                            Some(tag_ids)
                        },
                        external_id: None,
                    });
                }
            }

            if !linkage_updates.is_empty() {
                for chunk in linkage_updates.chunks(500) {
                    let mut chunk_txs = chunk.to_vec();
                    for u in &mut chunk_txs {
                        let is_loan = manual_accounts
                            .iter()
                            .find(|acc| target_accounts.get(&u.currency).copied() == Some(acc.id))
                            .map(|acc| acc.account_type == AccountType::Loan)
                            .unwrap_or(false);
                        if is_loan {
                            u.amount = -u.amount;
                        }
                    }
                    lm_client.update_transactions(&chunk_txs).await?;
                }
            }
        }
    }

    if !plan.deletes.is_empty() || !plan.updates.is_empty() || !plan.inserts.is_empty() {
        println! { "{STYLE_SUCCESS}✨ Synchronization cycle complete!{STYLE_SUCCESS:#}" };
        println! {};
    }

    Ok(())
}
