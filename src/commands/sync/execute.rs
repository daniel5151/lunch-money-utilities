use crate::api::lunch_money::schema::AccountType;
use crate::api::lunch_money::schema::ManualAccount;
use crate::style::*;
use anstream::println;
use rust_decimal::Decimal;
use std::collections::HashMap;

pub struct ApplySyncPlanArgs<'a> {
    pub plan: &'a mut super::SyncPlan,
    pub lm_client: &'a crate::api::lunch_money::Client,
    pub manual_accounts: &'a [ManualAccount],
    pub target_accounts: &'a HashMap<crate::api::Currency, u64>,
    pub tag_name: Option<&'a str>,
    pub loan_tag_name: Option<&'a str>,
    pub updated_tag_id: Option<u64>,
    pub lm_transactions: &'a [crate::api::lunch_money::schema::Transaction],
}

pub async fn apply_sync_plan(args: ApplySyncPlanArgs<'_>) -> anyhow::Result<()> {
    let ApplySyncPlanArgs {
        plan,
        lm_client,
        manual_accounts,
        target_accounts,
        tag_name,
        loan_tag_name,
        updated_tag_id,
        lm_transactions,
    } = args;

    let mut tag_id_map = HashMap::new();
    for name in &plan.tags_to_create {
        println! { "  {STYLE_DIM}Creating new tag '{}'...{STYLE_DIM:#}", name };
        let new_tag = lm_client.create_tag(name).await?;
        tag_id_map.insert(name.clone(), new_tag.id);
    }

    let created_tag_id = tag_name.and_then(|name| tag_id_map.get(name).copied());
    let created_loan_tag_id = loan_tag_name.and_then(|name| tag_id_map.get(name).copied());

    if created_tag_id.is_some() || created_loan_tag_id.is_some() {
        for ins in &mut plan.inserts {
            let mut ids = ins.tag_ids.take().unwrap_or_default();
            if let Some(id) = created_tag_id {
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
            if ins.amount > Decimal::ZERO {
                if let Some(id) = created_loan_tag_id {
                    if !ids.contains(&id) {
                        ids.push(id);
                    }
                }
            }
            if !ids.is_empty() {
                ins.tag_ids = Some(ids);
            }
        }
    }

    if !plan.deletes.is_empty() {
        let delete_ids: Vec<u64> = plan.deletes.iter().map(|t| t.id).collect();
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
                    },
                )) = inserted_tx.custom_metadata
                {
                    delta_inserts.push((original_transaction_id, inserted_tx.id));
                }
            }
        }

        if !delta_inserts.is_empty() {
            let mut linkage_updates = Vec::new();
            let mut deltas_by_orig: HashMap<u64, Vec<u64>> = HashMap::new();
            for (orig_id, delta_id) in delta_inserts {
                deltas_by_orig.entry(orig_id).or_default().push(delta_id);
            }

            for (orig_id, new_delta_ids) in deltas_by_orig {
                if let Some(orig_tx) = lm_transactions.iter().find(|t| t.id == orig_id) {
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

                    let next_delta_id = new_delta_ids[0];
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
                            delta_transaction_ids: updated_delta_ids,
                            original: original_expense,
                        };

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
                        notes: if updated_tag_id.is_some() {
                            super::format_notes_with_pointer(
                                &orig_tx.notes.clone().unwrap_or_default(),
                                next_delta_id,
                            )
                        } else {
                            orig_tx.notes.clone().unwrap_or_default()
                        },
                        custom_metadata: Some(desired_metadata),
                        additional_tag_ids: if tag_ids.is_empty() {
                            None
                        } else {
                            Some(tag_ids)
                        },
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
