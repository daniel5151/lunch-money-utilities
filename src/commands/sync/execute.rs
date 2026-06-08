use crate::api::LunchMoneyService;
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
}

pub async fn apply_sync_plan(args: ApplySyncPlanArgs<'_>) -> anyhow::Result<()> {
    let ApplySyncPlanArgs {
        plan,
        lm_client,
        manual_accounts,
        target_accounts,
        tag_name,
        loan_tag_name,
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
            lm_client.insert_transactions(&chunk_txs).await?;
        }
    }

    if !plan.deletes.is_empty() || !plan.updates.is_empty() || !plan.inserts.is_empty() {
        println! { "{STYLE_SUCCESS}✨ Synchronization cycle complete!{STYLE_SUCCESS:#}" };
        println! {};
    }

    Ok(())
}
