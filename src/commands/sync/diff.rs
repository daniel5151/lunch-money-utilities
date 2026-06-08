use super::SyncPlan;
use crate::api::Currency;
use crate::api::ExternalId;
use crate::api::lunch_money::schema::InsertObject;
use crate::api::lunch_money::schema::Transaction;
use crate::api::lunch_money::schema::UpdateObject;
use crate::api::splitwise::schema::Expense;
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
    } = args;
    let mut inserts = Vec::new();
    let mut updates = Vec::new();
    let mut deletes = Vec::new();

    for expense in expenses {
        let external_id = ExternalId::Splitwise(expense.id);

        let net_balance = expense
            .users
            .iter()
            .find(|u| u.user_id == config.splitwise.user_id)
            .map(|u| u.net_balance)
            .unwrap_or(Decimal::ZERO);

        let is_ignored = !bypass_ignore_groups
            && expense.group_id.is_some_and(|gid| {
                let name = group_map.get(&gid).map(|s| s.as_str());
                config.splitwise.is_group_ignored(gid, name) && Some(gid) != ignored_groups_exclude
            });

        // Skip ignored, deleted, or un-involved expenses
        if expense.deleted_at.is_some() || is_ignored || net_balance.is_zero() {
            if let Some(existing_lm) = lm_map.remove(&external_id) {
                if existing_lm.is_split_parent != Some(true) {
                    deletes.push(existing_lm);
                }
            }
            continue;
        }

        if !target_accounts.contains_key(&expense.currency_code) {
            anyhow::bail!(
                "No manual account configured for currency '{}'.\n\
                Please set up an active 'Splitwise {}' manual account in Lunch Money or configure [lunch_money.custom_accounts].",
                expense.currency_code,
                expense.currency_code
            );
        }

        let date_civil = expense.date.to_zoned(jiff::tz::TimeZone::UTC).date();

        let payee_str = if expense.group_id.is_none() {
            crate::commands::resolve_splitwise_payee(&expense, config.splitwise.user_id, group_map)
        } else {
            format!(
                "Splitwise - {}",
                crate::commands::resolve_splitwise_payee(
                    &expense,
                    config.splitwise.user_id,
                    group_map
                )
            )
        };

        if let Some(existing_lm) = lm_map.remove(&external_id) {
            if existing_lm.is_split_parent == Some(true) {
                continue;
            }
            let amount_changed = existing_lm.amount != net_balance;

            if amount_changed || existing_lm.currency != expense.currency_code {
                updates.push(UpdateObject {
                    id: existing_lm.id,
                    date: existing_lm.date,
                    amount: net_balance,
                    currency: expense.currency_code.clone(),
                    payee: existing_lm.payee.clone(),
                    notes: existing_lm.notes.clone().unwrap_or_default(),
                });
            }
        } else {
            let manual_account_id = target_accounts[&expense.currency_code];
            let mut category_id = None;
            if force_category_id.is_some() {
                category_id = force_category_id;
            } else if expense.payment {
                category_id = resolved_categories.get("Payment").copied();
            } else if let Some(ref cat) = expense.category {
                let path = sw_category_id_to_path.get(&cat.id);
                category_id = path
                    .and_then(|p| resolved_categories.get(p))
                    .or_else(|| resolved_categories.get(&cat.name))
                    .or_else(|| resolved_categories.get(&cat.id.to_string()))
                    .copied();
            }

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
                currency: expense.currency_code.clone(),
                payee: payee_str,
                notes: expense.description,
                external_id,
                manual_account_id,
                status: crate::api::lunch_money::schema::TransactionStatus::Unreviewed,
                tag_ids: tag_ids_opt,
                category_id,
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
        })
        .unwrap();
        let inserts2 = plan2.inserts;
        assert_eq!(inserts2.len(), 1);
    }
}
