use super::SyncPlan;
use crate::api::Currency;
use crate::api::ExternalId;
use crate::api::lunch_money::schema::InsertObject;
use crate::api::lunch_money::schema::Transaction;
use crate::api::lunch_money::schema::UpdateObject;
use crate::api::splitwise::Expense;
use crate::metadata::LunchMoneyTxMetadata;
use rust_decimal::Decimal;
use std::collections::HashMap;

pub struct DiffTransactionsArgs<'a> {
    pub expenses: Vec<Expense>,
    pub config: &'a crate::config::Config,
    pub target_accounts: &'a HashMap<Currency, u64>,
    pub group_map: &'a HashMap<u64, String>,
    pub lm_map: &'a mut HashMap<ExternalId, Transaction>,
    pub sw_category_id_to_path: &'a HashMap<u32, String>,
    pub resolved_categories: &'a HashMap<String, u64>,
    pub ignored_groups_exclude: Option<u64>,
    pub bypass_ignore_groups: bool,
    pub tag_id: Option<u64>,
    pub loan_tag_id: Option<u64>,
    pub force_category_id: Option<u64>,
    pub tags_to_create: Vec<String>,
    pub sync_window_start: Option<jiff::civil::Date>,
    pub backdated_tag_id: Option<u64>,
    pub updated_tag_id: Option<u64>,
}

fn resolve_category_for_expense(
    expense: &Expense,
    force_category_id: Option<u64>,
    resolved_categories: &HashMap<String, u64>,
    sw_category_id_to_path: &HashMap<u32, String>,
) -> Option<u64> {
    if force_category_id.is_some() {
        force_category_id
    } else if expense.parsed.payment {
        resolved_categories.get("Payment").copied()
    } else if let Some(ref cat) = expense.parsed.category {
        let path = sw_category_id_to_path.get(&cat.id);
        path.and_then(|p| resolved_categories.get(p))
            .or_else(|| resolved_categories.get(&cat.name))
            .or_else(|| resolved_categories.get(&cat.id.to_string()))
            .copied()
    } else {
        None
    }
}

fn apply_lpp_delta_engine(
    existing_lm: &Transaction,
    target_amount: Decimal,
    expense: &Expense,
    lm_by_id: &HashMap<u64, Transaction>,
    updates: &mut Vec<UpdateObject>,
    inserts: &mut Vec<InsertObject>,
    _config: &crate::config::Config,
    target_accounts: &HashMap<Currency, u64>,
    backdated_tag_id: Option<u64>,
    updated_tag_id: Option<u64>,
    _original_date: jiff::civil::Date,
    payee_str: &str,
    sync_window_start: Option<jiff::civil::Date>,
) -> anyhow::Result<()> {
    // 1. Fetch the original transaction metadata
    let LunchMoneyTxMetadata::Import {
        delta_transaction_ids,
        ..
    } = (match &existing_lm.custom_metadata {
        Some(crate::api::lunch_money::schema::MaybeLunchMoneyTxMetadata::Expected(metadata)) => {
            metadata
        }
        _ => anyhow::bail!(
            "Expected metadata on existing Splitwise transaction (ID: {})",
            existing_lm.id
        ),
    })
    else {
        anyhow::bail!(
            "Expected Import metadata kind on original transaction (ID: {})",
            existing_lm.id
        );
    };

    // 2. Fetch all delta transactions in the list from our in-memory map
    let mut delta_txs = Vec::new();
    for &d_id in delta_transaction_ids {
        if let Some(d_tx) = lm_by_id.get(&d_id) {
            delta_txs.push(d_tx.clone());
        }
    }

    // Sort delta transactions by date to find the latest
    delta_txs.sort_by_key(|t| t.date);

    // Sum of all existing entries
    let original_amount = existing_lm.amount;
    let sum_deltas: Decimal = delta_txs.iter().map(|t| t.amount).sum();
    let current_sum = original_amount + sum_deltas;

    // Check if the latest delta transaction date falls within the sync window (LPP)
    let latest_delta_in_lpp = if let Some(latest) = delta_txs.last() {
        sync_window_start.is_some_and(|ws| latest.date >= ws)
    } else {
        false
    };

    if latest_delta_in_lpp {
        // We update the latest delta transaction in-place.
        let latest = delta_txs.last().unwrap();
        let sum_excluding_latest = current_sum - latest.amount;
        let new_delta = target_amount - sum_excluding_latest;

        if new_delta != latest.amount {
            updates.push(UpdateObject {
                id: latest.id,
                date: latest.date,
                amount: new_delta,
                currency: latest.currency.clone(),
                payee: latest.payee.clone(),
                notes: latest.notes.clone().unwrap_or_default(),
                custom_metadata: Some(LunchMoneyTxMetadata::Delta {
                    original_transaction_id: existing_lm.id,
                }),
                additional_tag_ids: None,
            });

            // Also ensure the original transaction has the updated tag
            if let Some(ut_id) = updated_tag_id {
                updates.push(UpdateObject {
                    id: existing_lm.id,
                    date: existing_lm.date,
                    amount: existing_lm.amount,
                    currency: existing_lm.currency.clone(),
                    payee: existing_lm.payee.clone(),
                    notes: super::format_notes_with_pointer(
                        &existing_lm.notes.clone().unwrap_or_default(),
                        latest.id,
                    ),
                    custom_metadata: Some(LunchMoneyTxMetadata::Import {
                        delta_transaction_ids: delta_transaction_ids.clone(),
                        original: expense.parsed.clone().into(),
                    }),
                    additional_tag_ids: Some(vec![ut_id]),
                });
            }
        }
    } else {
        // We create a new delta transaction on the current day.
        let new_delta = target_amount - current_sum;

        if !new_delta.is_zero() {
            let manual_account_id = target_accounts[&expense.parsed.currency_code];
            let mut tag_ids = Vec::new();
            if let Some(bt_id) = backdated_tag_id {
                tag_ids.push(bt_id);
            }
            let tag_ids_opt = if tag_ids.is_empty() {
                None
            } else {
                Some(tag_ids)
            };

            let next_index = delta_txs.len();

            inserts.push(InsertObject {
                date: jiff::Timestamp::now()
                    .to_zoned(jiff::tz::TimeZone::UTC)
                    .date(),
                amount: new_delta,
                currency: expense.parsed.currency_code.clone(),
                payee: payee_str.to_string(),
                notes: format!(
                    "(Original Transaction: {}) {}",
                    existing_lm.id, expense.parsed.description
                ),
                external_id: ExternalId::SplitwiseDelta(expense.parsed.id, next_index),
                manual_account_id,
                status: crate::api::lunch_money::schema::TransactionStatus::Unreviewed,
                tag_ids: tag_ids_opt,
                category_id: None,
                custom_metadata: Some(LunchMoneyTxMetadata::Delta {
                    original_transaction_id: existing_lm.id,
                }),
            });
        }
    }

    Ok(())
}

pub fn diff_transactions(args: DiffTransactionsArgs<'_>) -> anyhow::Result<SyncPlan> {
    let DiffTransactionsArgs {
        expenses,
        config,
        target_accounts,
        group_map,
        lm_map,
        sw_category_id_to_path,
        resolved_categories,
        ignored_groups_exclude,
        bypass_ignore_groups,
        tag_id,
        loan_tag_id,
        force_category_id,
        tags_to_create,
        sync_window_start,
        backdated_tag_id,
        updated_tag_id,
    } = args;
    let mut inserts = Vec::new();
    let mut updates = Vec::new();
    let mut deletes = Vec::new();

    // Build helper map of all Lunch Money transactions by system ID to resolve delta chains
    let mut lm_by_id = HashMap::new();
    for t in lm_map.values() {
        lm_by_id.insert(t.id, t.clone());
    }

    for expense in expenses {
        let external_id = ExternalId::Splitwise(expense.parsed.id);

        let net_balance = expense
            .parsed
            .users
            .iter()
            .find(|u| u.user_id == config.splitwise.user_id)
            .map(|u| u.net_balance)
            .unwrap_or(Decimal::ZERO);

        let is_ignored = !bypass_ignore_groups
            && expense.parsed.group_id.is_some_and(|gid| {
                let name = group_map.get(&gid).map(|s| s.as_str());
                config.splitwise.is_group_ignored(gid, name) && Some(gid) != ignored_groups_exclude
            });

        let date_civil = expense.parsed.date.to_zoned(jiff::tz::TimeZone::UTC).date();
        let existing_opt = lm_map.remove(&external_id);
        let is_old = if let Some(ref existing_lm) = existing_opt {
            sync_window_start.is_some_and(|ws| existing_lm.date < ws)
        } else {
            sync_window_start.is_some_and(|ws| date_civil < ws)
        };
        let is_deleted_or_uninvolved =
            expense.parsed.deleted_at.is_some() || is_ignored || net_balance.is_zero();

        // Standard logic for in-window skips/deletes
        if !is_old && is_deleted_or_uninvolved {
            if let Some(existing_lm) = existing_opt {
                if existing_lm.is_split_parent != Some(true) {
                    deletes.push(existing_lm);
                }
            }
            continue;
        }

        if !target_accounts.contains_key(&expense.parsed.currency_code) {
            anyhow::bail!(
                "No manual account configured for currency '{}'.\n\
                Please set up an active 'Splitwise {}' manual account in Lunch Money or configure [lunch_money.custom_accounts].",
                expense.parsed.currency_code,
                expense.parsed.currency_code
            );
        }

        let payee_str = if expense.parsed.group_id.is_none() {
            crate::commands::resolve_splitwise_payee(
                &expense.parsed,
                config.splitwise.user_id,
                group_map,
            )
        } else {
            format!(
                "Splitwise - {}",
                crate::commands::resolve_splitwise_payee(
                    &expense.parsed,
                    config.splitwise.user_id,
                    group_map
                )
            )
        };

        if is_old {
            let target_amount = if is_deleted_or_uninvolved {
                Decimal::ZERO
            } else {
                net_balance
            };
            let currency_changed = existing_opt
                .as_ref()
                .is_some_and(|e| e.currency != expense.parsed.currency_code);

            if currency_changed {
                let old_lm = existing_opt.unwrap();
                apply_lpp_delta_engine(
                    &old_lm,
                    Decimal::ZERO,
                    &expense,
                    &lm_by_id,
                    &mut updates,
                    &mut inserts,
                    config,
                    target_accounts,
                    backdated_tag_id,
                    updated_tag_id,
                    date_civil,
                    &payee_str,
                    sync_window_start,
                )?;

                if !target_amount.is_zero() {
                    let manual_account_id = target_accounts[&expense.parsed.currency_code];
                    let mut tag_ids = Vec::new();
                    if let Some(bt_id) = backdated_tag_id {
                        tag_ids.push(bt_id);
                    }
                    if let Some(tid) = tag_id {
                        tag_ids.push(tid);
                    }
                    if target_amount > Decimal::ZERO {
                        if let Some(ltid) = loan_tag_id {
                            tag_ids.push(ltid);
                        }
                    }
                    let tag_ids_opt = if tag_ids.is_empty() {
                        None
                    } else {
                        Some(tag_ids)
                    };

                    let category_id = resolve_category_for_expense(
                        &expense,
                        force_category_id,
                        resolved_categories,
                        sw_category_id_to_path,
                    );

                    inserts.push(InsertObject {
                        date: jiff::Timestamp::now()
                            .to_zoned(jiff::tz::TimeZone::UTC)
                            .date(),
                        amount: target_amount,
                        currency: expense.parsed.currency_code.clone(),
                        payee: payee_str.clone(),
                        notes: format!(
                            "(Original Date: {}) {}",
                            date_civil, expense.parsed.description
                        ),
                        external_id: external_id.clone(),
                        manual_account_id,
                        status: crate::api::lunch_money::schema::TransactionStatus::Unreviewed,
                        tag_ids: tag_ids_opt,
                        category_id,
                        custom_metadata: Some(LunchMoneyTxMetadata::Import {
                            delta_transaction_ids: Vec::new(),
                            original: expense.parsed.clone().into(),
                        }),
                    });
                }
            } else if let Some(existing_lm) = existing_opt {
                apply_lpp_delta_engine(
                    &existing_lm,
                    target_amount,
                    &expense,
                    &lm_by_id,
                    &mut updates,
                    &mut inserts,
                    config,
                    target_accounts,
                    backdated_tag_id,
                    updated_tag_id,
                    date_civil,
                    &payee_str,
                    sync_window_start,
                )?;
            } else if !target_amount.is_zero() {
                let manual_account_id = target_accounts[&expense.parsed.currency_code];
                let mut tag_ids = Vec::new();
                if let Some(bt_id) = backdated_tag_id {
                    tag_ids.push(bt_id);
                }
                if let Some(tid) = tag_id {
                    tag_ids.push(tid);
                }
                if target_amount > Decimal::ZERO {
                    if let Some(ltid) = loan_tag_id {
                        tag_ids.push(ltid);
                    }
                }
                let tag_ids_opt = if tag_ids.is_empty() {
                    None
                } else {
                    Some(tag_ids)
                };

                let category_id = resolve_category_for_expense(
                    &expense,
                    force_category_id,
                    resolved_categories,
                    sw_category_id_to_path,
                );

                inserts.push(InsertObject {
                    date: jiff::Timestamp::now()
                        .to_zoned(jiff::tz::TimeZone::UTC)
                        .date(),
                    amount: target_amount,
                    currency: expense.parsed.currency_code.clone(),
                    payee: payee_str.clone(),
                    notes: format!(
                        "(Original Date: {}) {}",
                        date_civil, expense.parsed.description
                    ),
                    external_id: external_id.clone(),
                    manual_account_id,
                    status: crate::api::lunch_money::schema::TransactionStatus::Unreviewed,
                    tag_ids: tag_ids_opt,
                    category_id,
                    custom_metadata: Some(LunchMoneyTxMetadata::Import {
                        delta_transaction_ids: Vec::new(),
                        original: expense.parsed.clone().into(),
                    }),
                });
            }
            continue;
        }

        let desired_metadata = LunchMoneyTxMetadata::Import {
            delta_transaction_ids: Vec::new(),
            original: expense.parsed.clone().into(),
        };

        let mut existing_lm_opt = existing_opt;
        if let Some(ref existing_lm) = existing_lm_opt {
            if existing_lm.is_split_parent == Some(true) {
                continue;
            }
            let currency_changed = existing_lm.currency != expense.parsed.currency_code;
            if currency_changed {
                deletes.push(existing_lm_opt.take().unwrap());
            }
        }

        if let Some(existing_lm) = existing_lm_opt {
            let amount_changed = existing_lm.amount != net_balance;

            if amount_changed {
                updates.push(UpdateObject {
                    id: existing_lm.id,
                    date: existing_lm.date,
                    amount: net_balance,
                    currency: expense.parsed.currency_code.clone(),
                    payee: existing_lm.payee.clone(),
                    notes: existing_lm.notes.clone().unwrap_or_default(),
                    custom_metadata: Some(desired_metadata),
                    additional_tag_ids: None,
                });
            }
        } else {
            let manual_account_id = target_accounts[&expense.parsed.currency_code];
            let category_id = resolve_category_for_expense(
                &expense,
                force_category_id,
                resolved_categories,
                sw_category_id_to_path,
            );

            let mut tx_tag_ids = Vec::new();
            if let Some(tid) = tag_id {
                tx_tag_ids.push(tid);
            }
            if net_balance > Decimal::ZERO {
                if let Some(ltid) = loan_tag_id {
                    tx_tag_ids.push(ltid);
                }
            }
            let tag_ids_opt = if tx_tag_ids.is_empty() {
                None
            } else {
                Some(tx_tag_ids)
            };

            inserts.push(InsertObject {
                date: date_civil,
                amount: net_balance,
                currency: expense.parsed.currency_code.clone(),
                payee: payee_str,
                notes: expense.parsed.description,
                external_id,
                manual_account_id,
                status: crate::api::lunch_money::schema::TransactionStatus::Unreviewed,
                tag_ids: tag_ids_opt,
                category_id,
                custom_metadata: Some(desired_metadata),
            });
        }
    }

    Ok(SyncPlan {
        inserts,
        updates,
        deletes,
        tags_to_create,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diff_transactions_loan_tag() {
        let config_str = r#"
            [splitwise]
            api_key = "dummy"
            user_id = 123
            ignored_groups = []

            [lunch_money]
            api_key = "dummy"
            custom_accounts = { USD = 999 }

            [sync]
            backdated_tag = "backdated"
            updated_tag = "updated"
        "#;
        let config: crate::config::Config = toml::from_str(config_str).unwrap();

        let expenses_json = r#"[
            {
                "id": 1,
                "description": "Positive Net Balance (folks owe me)",
                "date": "2026-06-06T12:00:00Z",
                "currency_code": "USD",
                "users": [
                    {
                        "user_id": 123,
                        "net_balance": "50.00"
                    }
                ],
                "payment": false
            },
            {
                "id": 2,
                "description": "Negative Net Balance (I owe folks)",
                "date": "2026-06-06T12:00:00Z",
                "currency_code": "USD",
                "users": [
                    {
                        "user_id": 123,
                        "net_balance": "-20.00"
                    }
                ],
                "payment": false
            }
        ]"#;
        let expenses: Vec<Expense> = serde_json::from_str(expenses_json).unwrap();

        let mut target_accounts = HashMap::new();
        target_accounts.insert(Currency::new("USD"), 999);

        let mut lm_map = HashMap::new();
        let sw_category_id_to_path = HashMap::new();
        let resolved_categories = HashMap::new();

        let plan = diff_transactions(DiffTransactionsArgs {
            expenses,
            config: &config,
            target_accounts: &target_accounts,
            group_map: &HashMap::new(),
            lm_map: &mut lm_map,
            sw_category_id_to_path: &sw_category_id_to_path,
            resolved_categories: &resolved_categories,
            ignored_groups_exclude: None,
            bypass_ignore_groups: false,
            tag_id: Some(444),
            loan_tag_id: Some(555),
            force_category_id: None,
            tags_to_create: vec![],
            sync_window_start: None,
            backdated_tag_id: None,
            updated_tag_id: None,
        })
        .unwrap();

        let inserts = plan.inserts;
        let updates = plan.updates;
        let deletes = plan.deletes;

        assert!(updates.is_empty());
        assert!(deletes.is_empty());
        assert_eq!(inserts.len(), 2);

        // Transaction 1: net_balance is 50.00 (positive). Should have both tags.
        let tx1 = inserts
            .iter()
            .find(|tx| tx.amount == Decimal::new(5000, 2))
            .unwrap();
        assert_eq!(tx1.tag_ids, Some(vec![444, 555]));

        // Transaction 2: net_balance is -20.00 (negative). Should only have tag_id.
        let tx2 = inserts
            .iter()
            .find(|tx| tx.amount == Decimal::new(-2000, 2))
            .unwrap();
        assert_eq!(tx2.tag_ids, Some(vec![444]));
    }

    #[test]
    fn test_diff_transactions_no_loan_tag() {
        let config_str = r#"
            [splitwise]
            api_key = "dummy"
            user_id = 123
            ignored_groups = []

            [lunch_money]
            api_key = "dummy"
            custom_accounts = { USD = 999 }

            [sync]
            backdated_tag = "backdated"
            updated_tag = "updated"
        "#;
        let config: crate::config::Config = toml::from_str(config_str).unwrap();

        let expenses_json = r#"[
            {
                "id": 1,
                "description": "Positive Net Balance (folks owe me)",
                "date": "2026-06-06T12:00:00Z",
                "currency_code": "USD",
                "users": [
                    {
                        "user_id": 123,
                        "net_balance": "50.00"
                    }
                ],
                "payment": false
            }
        ]"#;
        let expenses: Vec<Expense> = serde_json::from_str(expenses_json).unwrap();

        let mut target_accounts = HashMap::new();
        target_accounts.insert(Currency::new("USD"), 999);

        let mut lm_map = HashMap::new();
        let sw_category_id_to_path = HashMap::new();
        let resolved_categories = HashMap::new();

        // Pass None for loan_tag_id
        let plan = diff_transactions(DiffTransactionsArgs {
            expenses,
            config: &config,
            target_accounts: &target_accounts,
            group_map: &HashMap::new(),
            lm_map: &mut lm_map,
            sw_category_id_to_path: &sw_category_id_to_path,
            resolved_categories: &resolved_categories,
            ignored_groups_exclude: None,
            bypass_ignore_groups: false,
            tag_id: Some(444),
            loan_tag_id: None,
            force_category_id: None,
            tags_to_create: vec![],
            sync_window_start: None,
            backdated_tag_id: None,
            updated_tag_id: None,
        })
        .unwrap();

        let inserts = plan.inserts;
        let updates = plan.updates;
        let deletes = plan.deletes;

        assert!(updates.is_empty());
        assert!(deletes.is_empty());
        assert_eq!(inserts.len(), 1);

        // Transaction 1: net_balance is 50.00 (positive). Should only have tag_id, not loan_tag_id.
        let tx1 = inserts
            .iter()
            .find(|tx| tx.amount == Decimal::new(5000, 2))
            .unwrap();
        assert_eq!(tx1.tag_ids, Some(vec![444]));
    }

    #[test]
    fn test_diff_transactions_force_category() {
        let config_str = r#"
            [splitwise]
            api_key = "dummy"
            user_id = 123
            ignored_groups = []

            [lunch_money]
            api_key = "dummy"
            custom_accounts = { USD = 999 }

            [sync]
            backdated_tag = "backdated"
            updated_tag = "updated"
        "#;
        let config: crate::config::Config = toml::from_str(config_str).unwrap();

        let expenses_json = r#"[
            {
                "id": 1,
                "description": "Any expense",
                "date": "2026-06-06T12:00:00Z",
                "currency_code": "USD",
                "users": [
                    {
                        "user_id": 123,
                        "net_balance": "10.00"
                    }
                ],
                "payment": false
            }
        ]"#;
        let expenses: Vec<Expense> = serde_json::from_str(expenses_json).unwrap();

        let mut target_accounts = HashMap::new();
        target_accounts.insert(Currency::new("USD"), 999);

        let mut lm_map = HashMap::new();
        let sw_category_id_to_path = HashMap::new();
        let resolved_categories = HashMap::new();

        let plan = diff_transactions(DiffTransactionsArgs {
            expenses,
            config: &config,
            target_accounts: &target_accounts,
            group_map: &HashMap::new(),
            lm_map: &mut lm_map,
            sw_category_id_to_path: &sw_category_id_to_path,
            resolved_categories: &resolved_categories,
            ignored_groups_exclude: None,
            bypass_ignore_groups: false,
            tag_id: None,
            loan_tag_id: None,
            force_category_id: Some(1010),
            tags_to_create: vec![],
            sync_window_start: None,
            backdated_tag_id: None,
            updated_tag_id: None,
        })
        .unwrap();

        let inserts = plan.inserts;
        assert_eq!(inserts.len(), 1);
        assert_eq!(inserts[0].category_id, Some(1010));
    }

    #[test]
    fn test_individual_payee_formatting() {
        let config_str = r#"
            [splitwise]
            api_key = "dummy"
            user_id = 123
            ignored_groups = []

            [lunch_money]
            api_key = "dummy"
            custom_accounts = { USD = 999 }

            [sync]
            backdated_tag = "backdated"
            updated_tag = "updated"
        "#;
        let config: crate::config::Config = toml::from_str(config_str).unwrap();

        let expenses_json = r#"[
            {
                "id": 1,
                "description": "Dinner expense",
                "date": "2026-06-06T12:00:00Z",
                "currency_code": "USD",
                "users": [
                    {
                        "user_id": 123,
                        "net_balance": "50.00"
                    },
                    {
                        "user_id": 456,
                        "net_balance": "-50.00",
                        "user": {
                            "first_name": "Alice",
                            "last_name": "Smith"
                        }
                    }
                ],
                "payment": false
            },
            {
                "id": 2,
                "group_id": 789,
                "description": "Group dinner",
                "date": "2026-06-06T12:00:00Z",
                "currency_code": "USD",
                "users": [
                    {
                        "user_id": 123,
                        "net_balance": "-20.00"
                    }
                ],
                "payment": false
            }
        ]"#;
        let expenses: Vec<Expense> = serde_json::from_str(expenses_json).unwrap();

        let mut target_accounts = HashMap::new();
        target_accounts.insert(Currency::new("USD"), 999);

        let mut lm_map = HashMap::new();
        let sw_category_id_to_path = HashMap::new();
        let resolved_categories = HashMap::new();

        let mut group_map = HashMap::new();
        group_map.insert(789, "Roommates".to_string());

        let plan = diff_transactions(DiffTransactionsArgs {
            expenses,
            config: &config,
            target_accounts: &target_accounts,
            group_map: &group_map,
            lm_map: &mut lm_map,
            sw_category_id_to_path: &sw_category_id_to_path,
            resolved_categories: &resolved_categories,
            ignored_groups_exclude: None,
            bypass_ignore_groups: false,
            tag_id: None,
            loan_tag_id: None,
            force_category_id: None,
            tags_to_create: vec![],
            sync_window_start: None,
            backdated_tag_id: None,
            updated_tag_id: None,
        })
        .unwrap();

        let inserts = plan.inserts;

        assert_eq!(inserts.len(), 2);

        // Individual expense (id: 1) should have payee "Alice Smith" (no "Splitwise - " prefix)
        let tx_individual = inserts
            .iter()
            .find(|tx| tx.external_id == ExternalId::Splitwise(1))
            .unwrap();
        assert_eq!(tx_individual.payee, "Alice Smith");

        // Group expense (id: 2) should have payee "Splitwise - Roommates"
        let tx_group = inserts
            .iter()
            .find(|tx| tx.external_id == ExternalId::Splitwise(2))
            .unwrap();
        assert_eq!(tx_group.payee, "Splitwise - Roommates");
    }

    #[test]
    fn test_diff_transactions_no_ignore() {
        let config_str = r#"
            [splitwise]
            api_key = "dummy"
            user_id = 123
            ignored_groups = [ 789 ]

            [lunch_money]
            api_key = "dummy"
            custom_accounts = { USD = 999 }

            [sync]
            backdated_tag = "backdated"
            updated_tag = "updated"
        "#;
        let config: crate::config::Config = toml::from_str(config_str).unwrap();

        let expenses_json = r#"[
            {
                "id": 1,
                "description": "Group expense",
                "date": "2026-06-06T12:00:00Z",
                "currency_code": "USD",
                "group_id": 789,
                "users": [
                    {
                        "user_id": 123,
                        "net_balance": "50.00"
                    }
                ],
                "payment": false
            }
        ]"#;
        let expenses1: Vec<Expense> = serde_json::from_str(expenses_json).unwrap();
        let expenses2: Vec<Expense> = serde_json::from_str(expenses_json).unwrap();

        let mut target_accounts = HashMap::new();
        target_accounts.insert(Currency::new("USD"), 999);

        let mut lm_map = HashMap::new();
        let sw_category_id_to_path = HashMap::new();
        let resolved_categories = HashMap::new();

        let mut group_map = HashMap::new();
        group_map.insert(789, "Roommates".to_string());

        // Case 1: bypass_ignore_groups = false (should be ignored, inserts is empty)
        let plan1 = diff_transactions(DiffTransactionsArgs {
            expenses: expenses1,
            config: &config,
            target_accounts: &target_accounts,
            group_map: &group_map,
            lm_map: &mut lm_map,
            sw_category_id_to_path: &sw_category_id_to_path,
            resolved_categories: &resolved_categories,
            ignored_groups_exclude: None,
            bypass_ignore_groups: false,
            tag_id: None,
            loan_tag_id: None,
            force_category_id: None,
            tags_to_create: vec![],
            sync_window_start: None,
            backdated_tag_id: None,
            updated_tag_id: None,
        })
        .unwrap();
        let inserts1 = plan1.inserts;
        assert!(inserts1.is_empty());

        // Case 2: bypass_ignore_groups = true (should NOT be ignored, inserts has 1 item)
        let plan2 = diff_transactions(DiffTransactionsArgs {
            expenses: expenses2,
            config: &config,
            target_accounts: &target_accounts,
            group_map: &group_map,
            lm_map: &mut lm_map,
            sw_category_id_to_path: &sw_category_id_to_path,
            resolved_categories: &resolved_categories,
            ignored_groups_exclude: None,
            bypass_ignore_groups: true,
            tag_id: None,
            loan_tag_id: None,
            force_category_id: None,
            tags_to_create: vec![],
            sync_window_start: None,
            backdated_tag_id: None,
            updated_tag_id: None,
        })
        .unwrap();
        let inserts2 = plan2.inserts;
        assert_eq!(inserts2.len(), 1);
    }

    #[test]
    fn test_diff_transactions_custom_metadata() {
        let config_str = r#"
            [splitwise]
            api_key = "dummy"
            user_id = 123
            ignored_groups = []

            [lunch_money]
            api_key = "dummy"
            custom_accounts = { USD = 999 }

            [sync]
            backdated_tag = "backdated"
            updated_tag = "updated"
        "#;
        let config: crate::config::Config = toml::from_str(config_str).unwrap();

        let expenses_json = r#"[
            {
                "id": 1,
                "description": "Positive Net Balance (folks owe me)",
                "date": "2026-06-06T12:00:00Z",
                "currency_code": "USD",
                "users": [
                    {
                        "user_id": 123,
                        "net_balance": "50.00"
                    }
                ],
                "payment": false
            }
        ]"#;
        let expenses: Vec<Expense> = serde_json::from_str(expenses_json).unwrap();
        let desired_metadata = LunchMoneyTxMetadata::Import {
            delta_transaction_ids: Vec::new(),
            original: expenses[0].parsed.clone().into(),
        };

        let mut target_accounts = HashMap::new();
        target_accounts.insert(Currency::new("USD"), 999);

        // Case 1: Existing transaction has no custom_metadata, but amount and currency are unchanged.
        // Should NOT trigger an update during diffing, because the sync command performs a fail-fast check earlier.
        let mut lm_map = HashMap::new();
        lm_map.insert(
            ExternalId::Splitwise(1),
            Transaction {
                id: 10,
                date: jiff::civil::date(2026, 6, 6),
                amount: Decimal::new(5000, 2),
                currency: Currency::new("USD"),
                payee: "Positive Net Balance (folks owe me)".to_string(),
                notes: None,
                external_id: Some(ExternalId::Splitwise(1)),
                manual_account_id: Some(999),
                is_split_parent: None,
                group_parent_id: None,
                status: crate::api::lunch_money::schema::TransactionStatus::Unreviewed,
                category_id: None,
                custom_metadata: None,
            },
        );

        let plan = diff_transactions(DiffTransactionsArgs {
            expenses: expenses.clone(),
            config: &config,
            target_accounts: &target_accounts,
            group_map: &HashMap::new(),
            lm_map: &mut lm_map,
            sw_category_id_to_path: &HashMap::new(),
            resolved_categories: &HashMap::new(),
            ignored_groups_exclude: None,
            bypass_ignore_groups: false,
            tag_id: None,
            loan_tag_id: None,
            force_category_id: None,
            tags_to_create: vec![],
            sync_window_start: None,
            backdated_tag_id: None,
            updated_tag_id: None,
        })
        .unwrap();

        assert!(plan.inserts.is_empty());
        assert!(plan.updates.is_empty());

        // Case 2: Existing transaction has identical custom_metadata. Should NOT trigger an update.
        let mut lm_map = HashMap::new();
        lm_map.insert(
            ExternalId::Splitwise(1),
            Transaction {
                id: 10,
                date: jiff::civil::date(2026, 6, 6),
                amount: Decimal::new(5000, 2),
                currency: Currency::new("USD"),
                payee: "Positive Net Balance (folks owe me)".to_string(),
                notes: None,
                external_id: Some(ExternalId::Splitwise(1)),
                manual_account_id: Some(999),
                is_split_parent: None,
                group_parent_id: None,
                status: crate::api::lunch_money::schema::TransactionStatus::Unreviewed,
                category_id: None,
                custom_metadata: Some(
                    crate::api::lunch_money::schema::MaybeLunchMoneyTxMetadata::Expected(
                        desired_metadata.clone(),
                    ),
                ),
            },
        );

        let plan = diff_transactions(DiffTransactionsArgs {
            expenses: expenses.clone(),
            config: &config,
            target_accounts: &target_accounts,
            group_map: &HashMap::new(),
            lm_map: &mut lm_map,
            sw_category_id_to_path: &HashMap::new(),
            resolved_categories: &HashMap::new(),
            ignored_groups_exclude: None,
            bypass_ignore_groups: false,
            tag_id: None,
            loan_tag_id: None,
            force_category_id: None,
            tags_to_create: vec![],
            sync_window_start: None,
            backdated_tag_id: None,
            updated_tag_id: None,
        })
        .unwrap();

        assert!(plan.inserts.is_empty());
        assert!(plan.updates.is_empty());

        // Case 3: Amount changed. Should trigger an update carrying custom_metadata.
        let mut lm_map = HashMap::new();
        lm_map.insert(
            ExternalId::Splitwise(1),
            Transaction {
                id: 10,
                date: jiff::civil::date(2026, 6, 6),
                amount: Decimal::new(4000, 2), // 40.00 instead of 50.00
                currency: Currency::new("USD"),
                payee: "Positive Net Balance (folks owe me)".to_string(),
                notes: None,
                external_id: Some(ExternalId::Splitwise(1)),
                manual_account_id: Some(999),
                is_split_parent: None,
                group_parent_id: None,
                status: crate::api::lunch_money::schema::TransactionStatus::Unreviewed,
                category_id: None,
                custom_metadata: Some(
                    crate::api::lunch_money::schema::MaybeLunchMoneyTxMetadata::Expected(
                        desired_metadata.clone(),
                    ),
                ),
            },
        );

        let plan = diff_transactions(DiffTransactionsArgs {
            expenses: expenses.clone(),
            config: &config,
            target_accounts: &target_accounts,
            group_map: &HashMap::new(),
            lm_map: &mut lm_map,
            sw_category_id_to_path: &HashMap::new(),
            resolved_categories: &HashMap::new(),
            ignored_groups_exclude: None,
            bypass_ignore_groups: false,
            tag_id: None,
            loan_tag_id: None,
            force_category_id: None,
            tags_to_create: vec![],
            sync_window_start: None,
            backdated_tag_id: None,
            updated_tag_id: None,
        })
        .unwrap();

        assert!(plan.inserts.is_empty());
        assert_eq!(plan.updates.len(), 1);
        assert_eq!(plan.updates[0].amount, Decimal::new(5000, 2));
        assert_eq!(plan.updates[0].custom_metadata, Some(desired_metadata));
    }

    #[test]
    fn test_backdated_sync_lpp_delta_engine() {
        let config_str = r#"
            [splitwise]
            api_key = "dummy"
            user_id = 123
            ignored_groups = []

            [lunch_money]
            api_key = "dummy"
            custom_accounts = { USD = 999 }

            [sync]
            backdated_tag = "backdated"
            updated_tag = "updated"
        "#;
        let config: crate::config::Config = toml::from_str(config_str).unwrap();

        // 1. Splitwise expense is dated 2026-05-01 (before sync window start: 2026-06-01)
        let expenses_json = r#"[
            {
                "id": 1,
                "description": "Lunch outside window",
                "date": "2026-05-01T12:00:00Z",
                "currency_code": "USD",
                "users": [
                    {
                        "user_id": 123,
                        "net_balance": "-15.00"
                    }
                ],
                "payment": false
            }
        ]"#;
        let expenses: Vec<Expense> = serde_json::from_str(expenses_json).unwrap();
        let original_expense = expenses[0].parsed.clone();

        let mut target_accounts = HashMap::new();
        target_accounts.insert(Currency::new("USD"), 999);

        // Case A: No existing delta transactions.
        // We have the original transaction (amount: -10.00). But the expense says -15.00.
        // It should insert a new delta transaction of -5.00 on the current day.
        let mut lm_map = HashMap::new();
        let orig_metadata = LunchMoneyTxMetadata::Import {
            delta_transaction_ids: Vec::new(),
            original: original_expense.clone().into(),
        };

        lm_map.insert(
            ExternalId::Splitwise(1),
            Transaction {
                id: 10,
                date: jiff::civil::date(2026, 5, 1),
                amount: Decimal::new(-1000, 2), // -10.00
                currency: Currency::new("USD"),
                payee: "Lunch outside window".to_string(),
                notes: None,
                external_id: Some(ExternalId::Splitwise(1)),
                manual_account_id: Some(999),
                is_split_parent: None,
                group_parent_id: None,
                status: crate::api::lunch_money::schema::TransactionStatus::Unreviewed,
                category_id: None,
                custom_metadata: Some(
                    crate::api::lunch_money::schema::MaybeLunchMoneyTxMetadata::Expected(
                        orig_metadata.clone(),
                    ),
                ),
            },
        );

        let plan = diff_transactions(DiffTransactionsArgs {
            expenses: expenses.clone(),
            config: &config,
            target_accounts: &target_accounts,
            group_map: &HashMap::new(),
            lm_map: &mut lm_map,
            sw_category_id_to_path: &HashMap::new(),
            resolved_categories: &HashMap::new(),
            ignored_groups_exclude: None,
            bypass_ignore_groups: false,
            tag_id: None,
            loan_tag_id: None,
            force_category_id: None,
            tags_to_create: vec![],
            sync_window_start: Some(jiff::civil::date(2026, 6, 1)),
            backdated_tag_id: Some(888),
            updated_tag_id: Some(777),
        })
        .unwrap();

        assert_eq!(plan.inserts.len(), 1);
        assert_eq!(plan.inserts[0].amount, Decimal::new(-500, 2)); // -5.00 delta
        assert_eq!(
            plan.inserts[0].external_id,
            ExternalId::SplitwiseDelta(1, 0)
        );
        assert_eq!(plan.inserts[0].tag_ids, Some(vec![888]));
        assert_eq!(
            plan.inserts[0].notes,
            "(Original Transaction: 10) Lunch outside window"
        );

        // Case B: There is an existing delta transaction outside LPP (dated 2026-05-15, which is before 2026-06-01).
        // Total so far: original (-10.00) + delta (-3.00) = -13.00. Target is -15.00.
        // It should insert a new delta transaction of -2.00 on the current day.
        let mut lm_map = HashMap::new();
        let orig_metadata_with_delta = LunchMoneyTxMetadata::Import {
            delta_transaction_ids: vec![20],
            original: original_expense.clone().into(),
        };

        lm_map.insert(
            ExternalId::Splitwise(1),
            Transaction {
                id: 10,
                date: jiff::civil::date(2026, 5, 1),
                amount: Decimal::new(-1000, 2), // -10.00
                currency: Currency::new("USD"),
                payee: "Lunch outside window".to_string(),
                notes: None,
                external_id: Some(ExternalId::Splitwise(1)),
                manual_account_id: Some(999),
                is_split_parent: None,
                group_parent_id: None,
                status: crate::api::lunch_money::schema::TransactionStatus::Unreviewed,
                category_id: None,
                custom_metadata: Some(
                    crate::api::lunch_money::schema::MaybeLunchMoneyTxMetadata::Expected(
                        orig_metadata_with_delta.clone(),
                    ),
                ),
            },
        );

        lm_map.insert(
            ExternalId::SplitwiseDelta(1, 0),
            Transaction {
                id: 20,
                date: jiff::civil::date(2026, 5, 15), // outside window
                amount: Decimal::new(-300, 2),        // -3.00
                currency: Currency::new("USD"),
                payee: "Lunch outside window delta".to_string(),
                notes: None,
                external_id: Some(ExternalId::SplitwiseDelta(1, 0)),
                manual_account_id: Some(999),
                is_split_parent: None,
                group_parent_id: None,
                status: crate::api::lunch_money::schema::TransactionStatus::Unreviewed,
                category_id: None,
                custom_metadata: Some(
                    crate::api::lunch_money::schema::MaybeLunchMoneyTxMetadata::Expected(
                        LunchMoneyTxMetadata::Delta {
                            original_transaction_id: 10,
                        },
                    ),
                ),
            },
        );

        let plan = diff_transactions(DiffTransactionsArgs {
            expenses: expenses.clone(),
            config: &config,
            target_accounts: &target_accounts,
            group_map: &HashMap::new(),
            lm_map: &mut lm_map,
            sw_category_id_to_path: &HashMap::new(),
            resolved_categories: &HashMap::new(),
            ignored_groups_exclude: None,
            bypass_ignore_groups: false,
            tag_id: None,
            loan_tag_id: None,
            force_category_id: None,
            tags_to_create: vec![],
            sync_window_start: Some(jiff::civil::date(2026, 6, 1)),
            backdated_tag_id: Some(888),
            updated_tag_id: Some(777),
        })
        .unwrap();

        assert_eq!(plan.inserts.len(), 1);
        assert_eq!(plan.inserts[0].amount, Decimal::new(-200, 2)); // -2.00 delta
        assert_eq!(
            plan.inserts[0].external_id,
            ExternalId::SplitwiseDelta(1, 1)
        );
        assert_eq!(
            plan.inserts[0].notes,
            "(Original Transaction: 10) Lunch outside window"
        );

        // Case C: There is an existing delta transaction inside LPP (dated 2026-06-05, which is after 2026-06-01).
        // Total so far: original (-10.00) + delta (-3.00) = -13.00. Target is -15.00.
        // It should update the existing delta transaction in-place to -5.00, rather than inserting a new one.
        let mut lm_map = HashMap::new();
        let orig_metadata_with_lpp_delta = LunchMoneyTxMetadata::Import {
            delta_transaction_ids: vec![20],
            original: original_expense.clone().into(),
        };

        lm_map.insert(
            ExternalId::Splitwise(1),
            Transaction {
                id: 10,
                date: jiff::civil::date(2026, 5, 1),
                amount: Decimal::new(-1000, 2), // -10.00
                currency: Currency::new("USD"),
                payee: "Lunch outside window".to_string(),
                notes: None,
                external_id: Some(ExternalId::Splitwise(1)),
                manual_account_id: Some(999),
                is_split_parent: None,
                group_parent_id: None,
                status: crate::api::lunch_money::schema::TransactionStatus::Unreviewed,
                category_id: None,
                custom_metadata: Some(
                    crate::api::lunch_money::schema::MaybeLunchMoneyTxMetadata::Expected(
                        orig_metadata_with_lpp_delta.clone(),
                    ),
                ),
            },
        );

        lm_map.insert(
            ExternalId::SplitwiseDelta(1, 0),
            Transaction {
                id: 20,
                date: jiff::civil::date(2026, 6, 5), // inside window (LPP)
                amount: Decimal::new(-300, 2),       // -3.00
                currency: Currency::new("USD"),
                payee: "Lunch outside window delta".to_string(),
                notes: None,
                external_id: Some(ExternalId::SplitwiseDelta(1, 0)),
                manual_account_id: Some(999),
                is_split_parent: None,
                group_parent_id: None,
                status: crate::api::lunch_money::schema::TransactionStatus::Unreviewed,
                category_id: None,
                custom_metadata: Some(
                    crate::api::lunch_money::schema::MaybeLunchMoneyTxMetadata::Expected(
                        LunchMoneyTxMetadata::Delta {
                            original_transaction_id: 10,
                        },
                    ),
                ),
            },
        );

        let plan = diff_transactions(DiffTransactionsArgs {
            expenses: expenses.clone(),
            config: &config,
            target_accounts: &target_accounts,
            group_map: &HashMap::new(),
            lm_map: &mut lm_map,
            sw_category_id_to_path: &HashMap::new(),
            resolved_categories: &HashMap::new(),
            ignored_groups_exclude: None,
            bypass_ignore_groups: false,
            tag_id: None,
            loan_tag_id: None,
            force_category_id: None,
            tags_to_create: vec![],
            sync_window_start: Some(jiff::civil::date(2026, 6, 1)),
            backdated_tag_id: Some(888),
            updated_tag_id: Some(777),
        })
        .unwrap();

        assert_eq!(plan.inserts.len(), 0);
        // It updates the delta transaction (ID 20) and adds the updated tag to the original transaction (ID 10)
        assert_eq!(plan.updates.len(), 2);

        let u_delta = plan.updates.iter().find(|u| u.id == 20).unwrap();
        assert_eq!(u_delta.amount, Decimal::new(-500, 2)); // -5.00 delta

        let u_orig = plan.updates.iter().find(|u| u.id == 10).unwrap();
        assert_eq!(u_orig.additional_tag_ids, Some(vec![777]));
        assert_eq!(u_orig.notes, "(See Transaction: 20)");

        // Case D: The backdated expense was already imported, and its Lunch Money transaction date is inside the sync window (e.g. 2026-06-05, which is after 2026-06-01).
        // Total so far: original (-10.00). Target is -15.00.
        // Since the Lunch Money transaction's posted date is within the LPP sync window, it should be updated in-place directly without delta creation.
        let mut lm_map = HashMap::new();
        let orig_metadata_in_window = LunchMoneyTxMetadata::Import {
            delta_transaction_ids: Vec::new(),
            original: original_expense.clone().into(),
        };

        lm_map.insert(
            ExternalId::Splitwise(1),
            Transaction {
                id: 10,
                date: jiff::civil::date(2026, 6, 5), // inside window (LPP)
                amount: Decimal::new(-1000, 2),      // -10.00
                currency: Currency::new("USD"),
                payee: "Lunch outside window".to_string(),
                notes: None,
                external_id: Some(ExternalId::Splitwise(1)),
                manual_account_id: Some(999),
                is_split_parent: None,
                group_parent_id: None,
                status: crate::api::lunch_money::schema::TransactionStatus::Unreviewed,
                category_id: None,
                custom_metadata: Some(
                    crate::api::lunch_money::schema::MaybeLunchMoneyTxMetadata::Expected(
                        orig_metadata_in_window.clone(),
                    ),
                ),
            },
        );

        let plan = diff_transactions(DiffTransactionsArgs {
            expenses: expenses.clone(),
            config: &config,
            target_accounts: &target_accounts,
            group_map: &HashMap::new(),
            lm_map: &mut lm_map,
            sw_category_id_to_path: &HashMap::new(),
            resolved_categories: &HashMap::new(),
            ignored_groups_exclude: None,
            bypass_ignore_groups: false,
            tag_id: None,
            loan_tag_id: None,
            force_category_id: None,
            tags_to_create: vec![],
            sync_window_start: Some(jiff::civil::date(2026, 6, 1)),
            backdated_tag_id: Some(888),
            updated_tag_id: Some(777),
        })
        .unwrap();

        assert_eq!(plan.inserts.len(), 0);
        assert_eq!(plan.updates.len(), 1);
        assert_eq!(plan.updates[0].id, 10);
        assert_eq!(plan.updates[0].amount, Decimal::new(-1500, 2)); // -15.00
    }

    #[test]
    fn test_diff_transactions_currency_changed_in_window() {
        let config_str = r#"
            [splitwise]
            api_key = "dummy"
            user_id = 123
            ignored_groups = []

            [lunch_money]
            api_key = "dummy"
            custom_accounts = { USD = 999, CAD = 888 }

            [sync]
            backdated_tag = "backdated"
            updated_tag = "updated"
        "#;
        let config: crate::config::Config = toml::from_str(config_str).unwrap();

        let expenses_json = r#"[
            {
                "id": 1,
                "description": "Lunch outside window",
                "date": "2026-06-06T12:00:00Z",
                "currency_code": "CAD",
                "users": [
                    {
                        "user_id": 123,
                        "net_balance": "-15.00"
                     }
                ],
                "payment": false
            }
        ]"#;
        let expenses: Vec<Expense> = serde_json::from_str(expenses_json).unwrap();

        let mut target_accounts = HashMap::new();
        target_accounts.insert(Currency::new("USD"), 999);
        target_accounts.insert(Currency::new("CAD"), 888);

        let mut lm_map = HashMap::new();
        let original_expense = expenses[0].parsed.clone();
        let orig_metadata = LunchMoneyTxMetadata::Import {
            delta_transaction_ids: Vec::new(),
            original: original_expense.into(),
        };

        // Existing transaction is USD in Lunch Money, but changed to CAD in Splitwise.
        lm_map.insert(
            ExternalId::Splitwise(1),
            Transaction {
                id: 10,
                date: jiff::civil::date(2026, 6, 6),
                amount: Decimal::new(-1500, 2),
                currency: Currency::new("USD"),
                payee: "Lunch outside window".to_string(),
                notes: None,
                external_id: Some(ExternalId::Splitwise(1)),
                manual_account_id: Some(999),
                is_split_parent: None,
                group_parent_id: None,
                status: crate::api::lunch_money::schema::TransactionStatus::Unreviewed,
                category_id: None,
                custom_metadata: Some(
                    crate::api::lunch_money::schema::MaybeLunchMoneyTxMetadata::Expected(
                        orig_metadata,
                    ),
                ),
            },
        );

        let plan = diff_transactions(DiffTransactionsArgs {
            expenses: expenses.clone(),
            config: &config,
            target_accounts: &target_accounts,
            group_map: &HashMap::new(),
            lm_map: &mut lm_map,
            sw_category_id_to_path: &HashMap::new(),
            resolved_categories: &HashMap::new(),
            ignored_groups_exclude: None,
            bypass_ignore_groups: false,
            tag_id: None,
            loan_tag_id: None,
            force_category_id: None,
            tags_to_create: vec![],
            sync_window_start: Some(jiff::civil::date(2026, 6, 1)),
            backdated_tag_id: Some(888),
            updated_tag_id: Some(777),
        })
        .unwrap();

        // Check that the old transaction is deleted
        assert_eq!(plan.deletes.len(), 1);
        assert_eq!(plan.deletes[0].id, 10);
        assert_eq!(plan.deletes[0].currency.as_str(), "USD");

        // Check that a new transaction is inserted in the CAD account
        assert_eq!(plan.inserts.len(), 1);
        assert_eq!(plan.inserts[0].currency.as_str(), "CAD");
        assert_eq!(plan.inserts[0].manual_account_id, 888);
        assert_eq!(plan.inserts[0].amount, Decimal::new(-1500, 2));

        // Updates should be empty
        assert_eq!(plan.updates.len(), 0);
    }
}
