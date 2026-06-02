use crate::api::lunch_money::schema::DeletePayload;
use crate::api::lunch_money::schema::InsertObject;
use crate::api::lunch_money::schema::InsertPayload;
use crate::api::lunch_money::schema::ManualAccountsResponse;
use crate::api::lunch_money::schema::Transaction;
use crate::api::lunch_money::schema::TransactionsResponse;
use crate::api::lunch_money::schema::UpdateObject;
use crate::api::lunch_money::schema::UpdatePayload;
use crate::api::splitwise::schema::ExpensesResponse;
use crate::api::splitwise::schema::GroupResponse;
use crate::style::*;
use reqwest::Method;
use rust_decimal::Decimal;
use std::collections::HashMap;

fn format_transaction_summary(
    payee: &str,
    amount: Decimal,
    currency: &str,
    date: jiff::civil::Date,
    notes: &str,
    account_name: &str,
) -> String {
    let date_str = date.strftime("%Y-%m-%d").to_string();
    let currency_upper = currency.to_uppercase();
    let amount_style = if amount.is_sign_negative() {
        STYLE_ERROR
    } else {
        STYLE_SUCCESS
    };

    // Limit payee length to 35 characters for clean alignment, appending '...' if truncated
    let mut clean_payee = payee.to_string();
    if clean_payee.chars().count() > 35 {
        clean_payee = clean_payee.chars().take(32).collect::<String>();
        clean_payee.push_str("...");
    }

    let trimmed_notes = notes.trim();
    let notes_suffix = if trimmed_notes.is_empty() {
        "".to_string()
    } else {
        format!("  {}{}{:#}", STYLE_DIM, trimmed_notes, STYLE_DIM)
    };

    let account_display = if account_name.is_empty() {
        "".to_string()
    } else {
        format!("  {}[{}]{:#}", STYLE_INFO, account_name, STYLE_INFO)
    };

    format!(
        "{}  {:<35}  {}{:>9} {}{:#}{}{}",
        date_str,
        clean_payee,
        amount_style,
        amount,
        currency_upper,
        amount_style,
        account_display,
        notes_suffix
    )
}

pub async fn run_sync_window(sync_args: crate::cli::SyncWindowArgs) {
    let window_duration =
        jiff::SignedDuration::try_from(sync_args.window).expect("window duration is too large");

    let config = crate::load_config();

    let http_pool = reqwest::Client::new();
    let sw_client =
        crate::api::splitwise::Client::new(http_pool.clone(), config.splitwise.api_key.clone());
    let lm_client =
        crate::api::lunch_money::Client::new(http_pool, config.lunch_money.api_key.clone());

    let start_window = jiff::Timestamp::now() - window_duration;
    let start_window_str = start_window
        .to_zoned(jiff::tz::TimeZone::UTC)
        .strftime("%Y-%m-%d")
        .to_string();

    let end_window_str = jiff::Timestamp::now()
        .to_zoned(jiff::tz::TimeZone::UTC)
        .strftime("%Y-%m-%d")
        .to_string();

    let dry_run_suffix = if sync_args.dry_run {
        format!(" {STYLE_WARNING}[DRY RUN]{STYLE_WARNING:#}")
    } else {
        "".to_string()
    };
    anstream::println! {};
    anstream::println! { "{STYLE_HEADER}⚡ Splitwise to Lunch Money Sync{}{STYLE_HEADER:#}", dry_run_suffix };
    anstream::println! { "{STYLE_DIM}──────────────────────────────────────────────────{STYLE_DIM:#}" };
    anstream::println! { "{STYLE_INFO}📅 Sync window boundary:{STYLE_INFO:#} {} to {}", start_window_str, end_window_str };
    anstream::println! {};

    // Fetch dependencies
    anstream::println! { "  {STYLE_DIM}Fetching Splitwise groups and expenses...{STYLE_DIM:#}" };
    let groups_res: GroupResponse = sw_client.fetch("get_groups", &[] as &[(&str, &str)]).await;
    let group_map: HashMap<u64, String> = groups_res
        .groups
        .into_iter()
        .map(|g| (g.id, g.name))
        .collect();

    let sw_query = [("dated_after", start_window_str.as_str()), ("limit", "0")];
    let expenses_res: ExpensesResponse = sw_client.fetch("get_expenses", &sw_query).await;

    // Verify configured manual accounts exist in Lunch Money
    let accounts_res: ManualAccountsResponse = lm_client
        .fetch("manual_accounts", &[] as &[(&str, &str)])
        .await;
    for (currency, &account_id) in &config.lunch_money.target_accounts {
        if !accounts_res
            .manual_accounts
            .iter()
            .any(|acc| acc.id == account_id)
        {
            anstream::eprintln! {};
            anstream::eprintln! { "{STYLE_ERROR}❌ Error:{STYLE_ERROR:#} Configured manual account ID {} for currency '{}' has been deleted or does not exist in Lunch Money.", account_id, currency };
            anstream::eprintln! { "Please check your Lunch Money manual accounts or run 'splitwise-lunchmoney init'." };
            anstream::eprintln! {};
            std::process::exit(1);
        }
    }

    let get_account_name = |manual_account_id: Option<u64>, currency: &str| -> String {
        let id = manual_account_id.or_else(|| {
            let currency_upper = currency.to_uppercase();
            config
                .lunch_money
                .target_accounts
                .get(&currency_upper)
                .copied()
        });
        if let Some(id) = id {
            if let Some(acc) = accounts_res.manual_accounts.iter().find(|acc| acc.id == id) {
                return acc.display_name.as_deref().unwrap_or(&acc.name).to_string();
            }
        }
        "Unknown Account".to_string()
    };

    anstream::println! { "  {STYLE_DIM}Fetching Lunch Money transactions...{STYLE_DIM:#}" };
    let mut lm_transactions = Vec::new();
    for &account_id in config.lunch_money.target_accounts.values() {
        let account_id_str = account_id.to_string();
        let lm_query = [
            ("start_date", start_window_str.as_str()),
            ("end_date", end_window_str.as_str()),
            ("manual_account_id", account_id_str.as_str()),
            ("limit", "1000"),
            ("include_group_children", "true"),
            ("include_split_parents", "true"),
        ];
        let lm_res: TransactionsResponse = lm_client.fetch("transactions", &lm_query).await;
        let is_loan = accounts_res
            .manual_accounts
            .iter()
            .find(|acc| acc.id == account_id)
            .map(|acc| acc.account_type == crate::api::lunch_money::schema::AccountType::Loan)
            .unwrap_or(false);

        let mut txs = lm_res.transactions;
        if is_loan {
            for t in &mut txs {
                t.amount = -t.amount;
            }
        }
        lm_transactions.extend(txs);
    }

    anstream::println! { "  {STYLE_DIM}Comparing transactions...{STYLE_DIM:#}" };
    anstream::println! {};

    // Theory of Operation (External IDs, Grouping, and Splitting):
    // 1. Transactions imported from Splitwise are tagged with a unique `external_id` matching `splitwise_<expense_id>`.
    // 2. We build `lm_map` only from Lunch Money transactions that have an `external_id`. Standard manual
    //    transactions or split/grouped artifacts without an `external_id` are ignored and untouched.
    // 3. When a user manually groups transactions in Lunch Money:
    //    - The new "group parent" transaction does not have our `external_id` and is ignored.
    //    - The "group child" transactions retain their `external_id`. By querying Lunch Money with
    //      `include_group_children=true`, they are fetched and successfully matched against Splitwise,
    //      preventing duplicate inserts.
    // 4. When a user manually splits a transaction in Lunch Money:
    //    - The "split parent" transaction keeps the `external_id`. By querying Lunch Money with
    //      `include_split_parents=true`, we fetch it. We explicitly skip updating it or deleting it.
    //    - The "split child" transactions do not have the matching `external_id`, so they are ignored
    //      by our sync engine (and are thus never modified or deleted).
    let mut lm_map: HashMap<String, Transaction> = lm_transactions
        .into_iter()
        .filter_map(|t| t.external_id.clone().map(|ext_id| (ext_id, t)))
        .collect();

    // Prepare batch operations
    let mut inserts: Vec<InsertObject> = Vec::new();
    let mut updates: Vec<UpdateObject> = Vec::new();
    let mut deletes: Vec<Transaction> = Vec::new();

    for expense in expenses_res.expenses {
        let external_id = format!("splitwise_{}", expense.id);

        let net_balance = expense
            .users
            .iter()
            .find(|u| u.user_id == config.splitwise.user_id)
            .map(|u| u.net_balance) // Automatically typed as Decimal by serde!
            .unwrap_or(Decimal::ZERO);

        let is_ignored = expense
            .group_id
            .is_some_and(|gid| config.splitwise.ignored_groups.contains(&gid));

        // Skip ignored, deleted, or un-involved expenses
        if expense.deleted_at.is_some() || is_ignored || net_balance.is_zero() {
            if let Some(existing_lm) = lm_map.remove(&external_id) {
                if existing_lm.is_split_parent != Some(true) {
                    deletes.push(existing_lm);
                }
            }
            continue;
        }

        let currency_upper = expense.currency_code.to_uppercase();
        if !config
            .lunch_money
            .target_accounts
            .contains_key(&currency_upper)
        {
            anstream::eprintln! {};
            anstream::eprintln! { "{STYLE_ERROR}❌ Error:{STYLE_ERROR:#} No manual account configured for currency '{}'.", currency_upper };
            anstream::eprintln! { "Please run 'splitwise-lunchmoney init' or set up 'Splitwise {}' manual account.", currency_upper };
            anstream::eprintln! {};
            std::process::exit(1);
        }

        let date_civil = expense.date.to_zoned(jiff::tz::TimeZone::UTC).date();
        let currency_lower = expense.currency_code.to_lowercase();

        let payee_str = format!(
            "Splitwise - {}",
            match expense.group_id {
                Some(gid) => group_map
                    .get(&gid)
                    .cloned()
                    .unwrap_or_else(|| "Unknown Group".to_string()),
                None => expense
                    .users
                    .iter()
                    .find(|u| u.user_id != config.splitwise.user_id)
                    .and_then(|u| u.user.as_ref())
                    .map(|d| {
                        format!(
                            "{} {}",
                            d.first_name.as_deref().unwrap_or(""),
                            d.last_name.as_deref().unwrap_or("")
                        )
                        .trim()
                        .to_string()
                    })
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| "Non-group".to_string()),
            }
        );

        if let Some(existing_lm) = lm_map.remove(&external_id) {
            if existing_lm.is_split_parent == Some(true) {
                continue;
            }
            // Strict exact-match diffing without float approximations
            let amount_changed = existing_lm.amount != net_balance;

            if amount_changed || existing_lm.currency != currency_lower {
                updates.push(UpdateObject {
                    id: existing_lm.id,
                    date: existing_lm.date,
                    amount: net_balance,
                    currency: currency_lower,
                    payee: existing_lm.payee.clone(),
                    notes: existing_lm.notes.clone().unwrap_or_default(),
                });
            }
        } else {
            let manual_account_id = config.lunch_money.target_accounts[&currency_upper];
            inserts.push(InsertObject {
                date: date_civil,
                amount: net_balance,
                currency: currency_lower,
                payee: payee_str,
                notes: expense.description,
                external_id,
                manual_account_id,
                status: crate::api::lunch_money::schema::TransactionStatus::Unreviewed,
                tag_ids: None,
            });
        }
    }

    // Execute batches
    if !deletes.is_empty() {
        anstream::println! { "🗑️  {STYLE_WARNING}Deleting {STYLE_WARNING:#}{} old/modified transaction(s) from Lunch Money:", deletes.len() };
        for t in &deletes {
            let acc_name = get_account_name(t.manual_account_id, &t.currency);
            anstream::println! { "   {STYLE_ERROR}-{STYLE_ERROR:#} {}", format_transaction_summary(&t.payee, t.amount, &t.currency, t.date, t.notes.as_deref().unwrap_or(""), &acc_name) };
        }
        anstream::println! {};

        if !sync_args.dry_run {
            let delete_ids: Vec<u64> = deletes.iter().map(|t| t.id).collect();
            lm_client
                .exec(
                    Method::DELETE,
                    "transactions",
                    &DeletePayload { ids: delete_ids },
                )
                .await;
        }
    }

    if !updates.is_empty() {
        anstream::println! { "✎  {STYLE_INFO}Updating {STYLE_INFO:#}{} modified transaction(s) in Lunch Money:", updates.len() };
        for u in &updates {
            let acc_name = get_account_name(None, &u.currency);
            anstream::println! { "   {STYLE_INFO}~{STYLE_INFO:#} {}", format_transaction_summary(&u.payee, u.amount, &u.currency, u.date, &u.notes, &acc_name) };
        }
        anstream::println! {};

        if !sync_args.dry_run {
            for chunk in updates.chunks(500) {
                let mut chunk_txs = chunk.to_vec();
                for u in &mut chunk_txs {
                    let is_loan = accounts_res
                        .manual_accounts
                        .iter()
                        .find(|acc| {
                            let curr = u.currency.to_uppercase();
                            config.lunch_money.target_accounts.get(&curr).copied() == Some(acc.id)
                        })
                        .map(|acc| {
                            acc.account_type == crate::api::lunch_money::schema::AccountType::Loan
                        })
                        .unwrap_or(false);
                    if is_loan {
                        u.amount = -u.amount;
                    }
                }
                lm_client
                    .exec(
                        Method::PUT,
                        "transactions",
                        &UpdatePayload {
                            transactions: chunk_txs,
                        },
                    )
                    .await;
            }
        }
    }

    if !inserts.is_empty() {
        anstream::println! { "✓  {STYLE_SUCCESS}Inserting {STYLE_SUCCESS:#}{} new transaction(s) to Lunch Money:", inserts.len() };
        for ins in &inserts {
            let acc_name = get_account_name(Some(ins.manual_account_id), &ins.currency);
            anstream::println! { "   {STYLE_SUCCESS}+{STYLE_SUCCESS:#} {}", format_transaction_summary(&ins.payee, ins.amount, &ins.currency, ins.date, &ins.notes, &acc_name) };
        }
        anstream::println! {};

        if !sync_args.dry_run {
            for chunk in inserts.chunks(500) {
                let mut chunk_txs = chunk.to_vec();
                for ins in &mut chunk_txs {
                    let is_loan = accounts_res
                        .manual_accounts
                        .iter()
                        .find(|acc| acc.id == ins.manual_account_id)
                        .map(|acc| {
                            acc.account_type == crate::api::lunch_money::schema::AccountType::Loan
                        })
                        .unwrap_or(false);
                    if is_loan {
                        ins.amount = -ins.amount;
                    }
                }
                lm_client
                    .exec(
                        Method::POST,
                        "transactions",
                        &InsertPayload {
                            transactions: chunk_txs,
                        },
                    )
                    .await;
            }
        }
    }

    if deletes.is_empty() && updates.is_empty() && inserts.is_empty() {
        anstream::println! { "{STYLE_SUCCESS}✨ No changes detected. Lunch Money manual account is up-to-date!{STYLE_SUCCESS:#}" };
        anstream::println! {};
    } else if sync_args.dry_run {
        anstream::println! { "{STYLE_WARNING}⚠️ Dry run complete! No changes were made to Lunch Money.{STYLE_WARNING:#}" };
        anstream::println! {};
    } else {
        anstream::println! { "{STYLE_SUCCESS}✨ Synchronization cycle complete!{STYLE_SUCCESS:#}" };
        anstream::println! {};
    }
}

pub async fn run_sync_group(sync_args: crate::cli::SyncGroupArgs) {
    let config = crate::load_config();

    let http_pool = reqwest::Client::new();
    let sw_client =
        crate::api::splitwise::Client::new(http_pool.clone(), config.splitwise.api_key.clone());
    let lm_client =
        crate::api::lunch_money::Client::new(http_pool, config.lunch_money.api_key.clone());

    let dry_run_suffix = if sync_args.dry_run {
        format!(" {STYLE_WARNING}[DRY RUN]{STYLE_WARNING:#}")
    } else {
        "".to_string()
    };
    anstream::println! {};
    anstream::println! { "{STYLE_HEADER}⚡ Splitwise to Lunch Money Sync Group{}{STYLE_HEADER:#}", dry_run_suffix };
    anstream::println! { "{STYLE_DIM}──────────────────────────────────────────────────{STYLE_DIM:#}" };

    // Fetch dependencies
    anstream::println! { "  {STYLE_DIM}Fetching Splitwise groups and expenses...{STYLE_DIM:#}" };
    let groups_res: GroupResponse = sw_client.fetch("get_groups", &[] as &[(&str, &str)]).await;
    let group_map: HashMap<u64, String> = groups_res
        .groups
        .iter()
        .map(|g| (g.id, g.name.clone()))
        .collect();

    let target_group = groups_res
        .groups
        .iter()
        .find(|g| g.id == sync_args.group_id);
    let group_name = target_group
        .map(|g| g.name.clone())
        .unwrap_or_else(|| "Unknown Group".to_string());

    anstream::println! { "{STYLE_INFO}👥 Group:{STYLE_INFO:#} {} (ID: {})", group_name, sync_args.group_id };
    if let Some(g) = target_group {
        let balance_str =
            crate::commands::query::format_group_balances(g, config.splitwise.user_id);
        anstream::println! { "{STYLE_INFO}💰 Balance:{STYLE_INFO:#} {}", balance_str };
    }
    anstream::println! {};

    let group_id_str = sync_args.group_id.to_string();
    let sw_query = [("group_id", group_id_str.as_str()), ("limit", "0")];
    let expenses_res: ExpensesResponse = sw_client.fetch("get_expenses", &sw_query).await;

    // Verify configured manual accounts exist in Lunch Money
    let accounts_res: ManualAccountsResponse = lm_client
        .fetch("manual_accounts", &[] as &[(&str, &str)])
        .await;
    for (currency, &account_id) in &config.lunch_money.target_accounts {
        if !accounts_res
            .manual_accounts
            .iter()
            .any(|acc| acc.id == account_id)
        {
            anstream::eprintln! {};
            anstream::eprintln! { "{STYLE_ERROR}❌ Error:{STYLE_ERROR:#} Configured manual account ID {} for currency '{}' has been deleted or does not exist in Lunch Money.", account_id, currency };
            anstream::eprintln! { "Please check your Lunch Money manual accounts or run 'splitwise-lunchmoney init'." };
            anstream::eprintln! {};
            std::process::exit(1);
        }
    }

    let mut tag_id = None;
    if let Some(ref tag_name) = sync_args.tag {
        anstream::println! { "  {STYLE_DIM}Resolving Lunch Money tag '{}'...{STYLE_DIM:#}", tag_name };
        let tags_res: crate::api::lunch_money::schema::TagsResponse =
            lm_client.fetch("tags", &[] as &[(&str, &str)]).await;

        if let Some(existing_tag) = tags_res
            .tags
            .iter()
            .find(|t| t.name.eq_ignore_ascii_case(tag_name))
        {
            tag_id = Some(existing_tag.id);
        } else {
            if sync_args.dry_run {
                anstream::println! { "   {STYLE_WARNING}Would create tag:{STYLE_WARNING:#} '{}'", tag_name };
                tag_id = Some(0);
            } else {
                anstream::println! { "  {STYLE_DIM}Creating new tag '{}'...{STYLE_DIM:#}", tag_name };
                let new_tag: crate::api::lunch_money::schema::Tag = lm_client
                    .exec_with_response(
                        Method::POST,
                        "tags",
                        &crate::api::lunch_money::schema::CreateTagPayload {
                            name: tag_name.clone(),
                        },
                    )
                    .await;
                tag_id = Some(new_tag.id);
            }
        }
    }

    let get_account_name = |manual_account_id: Option<u64>, currency: &str| -> String {
        let id = manual_account_id.or_else(|| {
            let currency_upper = currency.to_uppercase();
            config
                .lunch_money
                .target_accounts
                .get(&currency_upper)
                .copied()
        });
        if let Some(id) = id {
            if let Some(acc) = accounts_res.manual_accounts.iter().find(|acc| acc.id == id) {
                return acc.display_name.as_deref().unwrap_or(&acc.name).to_string();
            }
        }
        "Unknown Account".to_string()
    };

    anstream::println! { "  {STYLE_DIM}Fetching Lunch Money transactions...{STYLE_DIM:#}" };
    let end_window_str = jiff::Timestamp::now()
        .to_zoned(jiff::tz::TimeZone::UTC)
        .strftime("%Y-%m-%d")
        .to_string();
    let mut lm_transactions = Vec::new();
    for &account_id in config.lunch_money.target_accounts.values() {
        let account_id_str = account_id.to_string();
        let lm_query = [
            ("start_date", "2000-01-01"),
            ("end_date", end_window_str.as_str()),
            ("manual_account_id", account_id_str.as_str()),
            ("limit", "1000"),
            ("include_group_children", "true"),
            ("include_split_parents", "true"),
        ];
        let lm_res: TransactionsResponse = lm_client.fetch("transactions", &lm_query).await;
        let is_loan = accounts_res
            .manual_accounts
            .iter()
            .find(|acc| acc.id == account_id)
            .map(|acc| acc.account_type == crate::api::lunch_money::schema::AccountType::Loan)
            .unwrap_or(false);

        let mut txs = lm_res.transactions;
        if is_loan {
            for t in &mut txs {
                t.amount = -t.amount;
            }
        }
        lm_transactions.extend(txs);
    }

    anstream::println! { "  {STYLE_DIM}Comparing transactions...{STYLE_DIM:#}" };
    anstream::println! {};

    // Theory of Operation (External IDs, Grouping, and Splitting):
    // 1. Transactions imported from Splitwise are tagged with a unique `external_id` matching `splitwise_<expense_id>`.
    // 2. We build `lm_map` only from Lunch Money transactions that have an `external_id`. Standard manual
    //    transactions or split/grouped artifacts without an `external_id` are ignored and untouched.
    // 3. When a user manually groups transactions in Lunch Money:
    //    - The new "group parent" transaction does not have our `external_id` and is ignored.
    //    - The "group child" transactions retain their `external_id`. By querying Lunch Money with
    //      `include_group_children=true`, they are fetched and successfully matched against Splitwise,
    //      preventing duplicate inserts.
    // 4. When a user manually splits a transaction in Lunch Money:
    //    - The "split parent" transaction keeps the `external_id`. By querying Lunch Money with
    //      `include_split_parents=true`, we fetch it. We explicitly skip updating it or deleting it.
    //    - The "split child" transactions do not have the matching `external_id`, so they are ignored
    //      by our sync engine (and are thus never modified or deleted).
    let mut lm_map: HashMap<String, Transaction> = lm_transactions
        .into_iter()
        .filter_map(|t| t.external_id.clone().map(|ext_id| (ext_id, t)))
        .collect();

    // Prepare batch operations
    let mut inserts: Vec<InsertObject> = Vec::new();
    let mut updates: Vec<UpdateObject> = Vec::new();
    let mut deletes: Vec<Transaction> = Vec::new();

    for expense in expenses_res.expenses {
        let external_id = format!("splitwise_{}", expense.id);

        let net_balance = expense
            .users
            .iter()
            .find(|u| u.user_id == config.splitwise.user_id)
            .map(|u| u.net_balance)
            .unwrap_or(Decimal::ZERO);

        let is_ignored = expense
            .group_id
            .is_some_and(|gid| config.splitwise.ignored_groups.contains(&gid));

        // Skip ignored, deleted, or un-involved expenses
        if expense.deleted_at.is_some() || is_ignored || net_balance.is_zero() {
            if let Some(existing_lm) = lm_map.remove(&external_id) {
                if existing_lm.is_split_parent != Some(true) {
                    deletes.push(existing_lm);
                }
            }
            continue;
        }

        let currency_upper = expense.currency_code.to_uppercase();
        if !config
            .lunch_money
            .target_accounts
            .contains_key(&currency_upper)
        {
            anstream::eprintln! {};
            anstream::eprintln! { "{STYLE_ERROR}❌ Error:{STYLE_ERROR:#} No manual account configured for currency '{}'.", currency_upper };
            anstream::eprintln! { "Please run 'splitwise-lunchmoney init' or set up 'Splitwise {}' manual account.", currency_upper };
            anstream::eprintln! {};
            std::process::exit(1);
        }

        let date_civil = expense.date.to_zoned(jiff::tz::TimeZone::UTC).date();
        let currency_lower = expense.currency_code.to_lowercase();

        let payee_str = format!(
            "Splitwise - {}",
            match expense.group_id {
                Some(gid) => group_map
                    .get(&gid)
                    .cloned()
                    .unwrap_or_else(|| "Unknown Group".to_string()),
                None => expense
                    .users
                    .iter()
                    .find(|u| u.user_id != config.splitwise.user_id)
                    .and_then(|u| u.user.as_ref())
                    .map(|d| {
                        format!(
                            "{} {}",
                            d.first_name.as_deref().unwrap_or(""),
                            d.last_name.as_deref().unwrap_or("")
                        )
                        .trim()
                        .to_string()
                    })
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| "Non-group".to_string()),
            }
        );

        if let Some(existing_lm) = lm_map.remove(&external_id) {
            if existing_lm.is_split_parent == Some(true) {
                continue;
            }
            let amount_changed = existing_lm.amount != net_balance;

            if amount_changed || existing_lm.currency != currency_lower {
                updates.push(UpdateObject {
                    id: existing_lm.id,
                    date: existing_lm.date,
                    amount: net_balance,
                    currency: currency_lower,
                    payee: existing_lm.payee.clone(),
                    notes: existing_lm.notes.clone().unwrap_or_default(),
                });
            }
        } else {
            let manual_account_id = config.lunch_money.target_accounts[&currency_upper];
            inserts.push(InsertObject {
                date: date_civil,
                amount: net_balance,
                currency: currency_lower,
                payee: payee_str,
                notes: expense.description,
                external_id,
                manual_account_id,
                status: crate::api::lunch_money::schema::TransactionStatus::Unreviewed,
                tag_ids: tag_id.map(|id| vec![id]),
            });
        }
    }

    // Filter deletes to only target transactions belonging to this specific group
    let is_non_group = sync_args.group_id == 0;
    let group_payee = format!("Splitwise - {}", group_name);

    for (_ext_id, t) in lm_map {
        let belongs_to_group = if is_non_group {
            t.payee == "Splitwise - Non-group"
                || (!group_map
                    .values()
                    .any(|gn| t.payee == format!("Splitwise - {}", gn))
                    && t.payee.starts_with("Splitwise - "))
        } else {
            t.payee == group_payee
        };

        if belongs_to_group && t.is_split_parent != Some(true) {
            deletes.push(t);
        }
    }

    // Execute batches
    if !deletes.is_empty() {
        anstream::println! { "🗑️  {STYLE_WARNING}Deleting {STYLE_WARNING:#}{} old/modified transaction(s) from Lunch Money:", deletes.len() };
        for t in &deletes {
            let acc_name = get_account_name(t.manual_account_id, &t.currency);
            anstream::println! { "   {STYLE_ERROR}-{STYLE_ERROR:#} {}", format_transaction_summary(&t.payee, t.amount, &t.currency, t.date, t.notes.as_deref().unwrap_or(""), &acc_name) };
        }
        anstream::println! {};

        if !sync_args.dry_run {
            let delete_ids: Vec<u64> = deletes.iter().map(|t| t.id).collect();
            lm_client
                .exec(
                    Method::DELETE,
                    "transactions",
                    &DeletePayload { ids: delete_ids },
                )
                .await;
        }
    }

    if !updates.is_empty() {
        anstream::println! { "✎  {STYLE_INFO}Updating {STYLE_INFO:#}{} modified transaction(s) in Lunch Money:", updates.len() };
        for u in &updates {
            let acc_name = get_account_name(None, &u.currency);
            anstream::println! { "   {STYLE_INFO}~{STYLE_INFO:#} {}", format_transaction_summary(&u.payee, u.amount, &u.currency, u.date, &u.notes, &acc_name) };
        }
        anstream::println! {};

        if !sync_args.dry_run {
            for chunk in updates.chunks(500) {
                let mut chunk_txs = chunk.to_vec();
                for u in &mut chunk_txs {
                    let is_loan = accounts_res
                        .manual_accounts
                        .iter()
                        .find(|acc| {
                            let curr = u.currency.to_uppercase();
                            config.lunch_money.target_accounts.get(&curr).copied() == Some(acc.id)
                        })
                        .map(|acc| {
                            acc.account_type == crate::api::lunch_money::schema::AccountType::Loan
                        })
                        .unwrap_or(false);
                    if is_loan {
                        u.amount = -u.amount;
                    }
                }
                lm_client
                    .exec(
                        Method::PUT,
                        "transactions",
                        &UpdatePayload {
                            transactions: chunk_txs,
                        },
                    )
                    .await;
            }
        }
    }

    if !inserts.is_empty() {
        anstream::println! { "✓  {STYLE_SUCCESS}Inserting {STYLE_SUCCESS:#}{} new transaction(s) to Lunch Money:", inserts.len() };
        for ins in &inserts {
            let acc_name = get_account_name(Some(ins.manual_account_id), &ins.currency);
            anstream::println! { "   {STYLE_SUCCESS}+{STYLE_SUCCESS:#} {}", format_transaction_summary(&ins.payee, ins.amount, &ins.currency, ins.date, &ins.notes, &acc_name) };
        }
        anstream::println! {};

        if !sync_args.dry_run {
            for chunk in inserts.chunks(500) {
                let mut chunk_txs = chunk.to_vec();
                for ins in &mut chunk_txs {
                    let is_loan = accounts_res
                        .manual_accounts
                        .iter()
                        .find(|acc| acc.id == ins.manual_account_id)
                        .map(|acc| {
                            acc.account_type == crate::api::lunch_money::schema::AccountType::Loan
                        })
                        .unwrap_or(false);
                    if is_loan {
                        ins.amount = -ins.amount;
                    }
                }
                lm_client
                    .exec(
                        Method::POST,
                        "transactions",
                        &InsertPayload {
                            transactions: chunk_txs,
                        },
                    )
                    .await;
            }
        }
    }

    if deletes.is_empty() && updates.is_empty() && inserts.is_empty() {
        anstream::println! { "{STYLE_SUCCESS}✨ No changes detected. Lunch Money manual account is up-to-date!{STYLE_SUCCESS:#}" };
        anstream::println! {};
    } else if sync_args.dry_run {
        anstream::println! { "{STYLE_WARNING}⚠️ Dry run complete! No changes were made to Lunch Money.{STYLE_WARNING:#}" };
        anstream::println! {};
    } else {
        anstream::println! { "{STYLE_SUCCESS}✨ Synchronization cycle complete!{STYLE_SUCCESS:#}" };
        anstream::println! {};
    }
}
