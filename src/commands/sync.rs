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
use anstream::eprintln;
use anstream::println;
use reqwest::Method;
use rust_decimal::Decimal;
use std::collections::HashMap;

fn format_transaction_summary(
    payee: &str,
    amount: Decimal,
    currency: &crate::api::Currency,
    date: jiff::civil::Date,
    notes: &str,
    sw_category_name: Option<&str>,
    lm_category_name: Option<&str>,
    payee_width: usize,
    sw_cat_width: usize,
    lm_cat_width: usize,
) -> String {
    let date_str = date.strftime("%Y-%m-%d").to_string();
    let currency_upper = currency.to_uppercase();
    let amount_style = if amount.is_sign_negative() {
        STYLE_ERROR
    } else {
        STYLE_SUCCESS
    };

    // Limit payee length to 50 characters for clean alignment, appending '...' if truncated
    let mut clean_payee = payee.to_string();
    if clean_payee.starts_with("Splitwise - ") {
        clean_payee = clean_payee["Splitwise - ".len()..].to_string();
    }
    if clean_payee.chars().count() > 50 {
        clean_payee = clean_payee.chars().take(47).collect::<String>();
        clean_payee.push_str("...");
    }

    let trimmed_notes = notes.trim();
    let notes_suffix = if trimmed_notes.is_empty() {
        "".to_string()
    } else {
        format!("  {}{}{:#}", STYLE_DIM, trimmed_notes, STYLE_DIM)
    };

    // Format two separate columns for Splitwise and Lunch Money categories
    // Special case "Uncategorized:General" to not be printed at all
    let sw_clean = match sw_category_name {
        Some("Uncategorized:General") => None,
        other => other,
    };

    let category_display = if sw_clean.is_none() && lm_category_name.is_none() {
        let total_width = if sw_cat_width > 0 {
            sw_cat_width + 2
        } else {
            0
        } + lm_cat_width
            + 2;
        " ".repeat(total_width)
    } else {
        let sw_part = sw_clean.unwrap_or("");
        let lm_part = lm_category_name.unwrap_or("?");

        let sw_col = if sw_part.is_empty() {
            if sw_cat_width > 0 {
                format!("  {}", " ".repeat(sw_cat_width))
            } else {
                "".to_string()
            }
        } else {
            format!(
                "  {}{:<width$}{:#}",
                STYLE_WARNING,
                sw_part,
                STYLE_WARNING,
                width = sw_cat_width
            )
        };

        let lm_col = format!(
            "  {}{:<width$}{:#}",
            STYLE_WARNING,
            lm_part,
            STYLE_WARNING,
            width = lm_cat_width
        );

        format!("{}{}", sw_col, lm_col)
    };

    let line = format!(
        "{}  {:<width$}  {}{:>9} {}{:#}{}{}",
        date_str,
        clean_payee,
        amount_style,
        amount,
        currency_upper,
        amount_style,
        category_display,
        notes_suffix,
        width = payee_width
    );
    line.trim_end().to_string()
}

async fn resolve_categories(
    lm_client: &crate::api::lunch_money::Client,
    config: &crate::config::Config,
) -> (HashMap<String, u64>, HashMap<u64, String>) {
    if config.categories.is_empty() {
        return (HashMap::new(), HashMap::new());
    }

    println! { "  {STYLE_DIM}Fetching Lunch Money categories...{STYLE_DIM:#}" };
    let categories_res: crate::api::lunch_money::schema::CategoriesResponse = lm_client
        .fetch("categories", &[("format", "flattened")] as &[(&str, &str)])
        .await;

    let names: HashMap<u64, String> = categories_res
        .categories
        .iter()
        .map(|c| (c.id, c.name.clone()))
        .collect();

    let mut resolved = HashMap::new();
    for (sw_key, lm_val) in &config.categories {
        let resolved_id = match lm_val {
            crate::config::CategoryValue::Id(id) => {
                if categories_res
                    .categories
                    .iter()
                    .any(|c| c.id == *id && !c.archived)
                {
                    *id
                } else {
                    println! { "  ⚠️  {STYLE_WARNING}Warning:{STYLE_WARNING:#} Configured Lunch Money category ID {} (for Splitwise category '{}') does not exist or is archived.", id, sw_key };
                    continue;
                }
            }
            crate::config::CategoryValue::Name(name) => {
                let matches: Vec<_> = categories_res
                    .categories
                    .iter()
                    .filter(|c| c.name.eq_ignore_ascii_case(name) && !c.archived)
                    .collect();
                if matches.is_empty() {
                    println! { "  ⚠️  {STYLE_WARNING}Warning:{STYLE_WARNING:#} Configured Lunch Money category '{}' (for Splitwise category '{}') does not exist or is archived.", name, sw_key };
                    continue;
                } else if matches.len() > 1 {
                    eprintln! {};
                    eprintln! { "{STYLE_ERROR}❌ Error:{STYLE_ERROR:#} Multiple active Lunch Money categories found with the name '{}':", name };
                    for m in matches {
                        eprintln! { "  • ID: {} (is_group: {})", m.id, m.is_group };
                    }
                    eprintln! { "Please map by category ID instead to resolve ambiguity." };
                    eprintln! {};
                    std::process::exit(1);
                } else {
                    matches[0].id
                }
            }
        };

        resolved.insert(sw_key.clone(), resolved_id);
    }
    (resolved, names)
}

pub(crate) async fn run_sync_window(sync_args: crate::cli::SyncWindowArgs) {
    let window_duration =
        jiff::SignedDuration::try_from(sync_args.window).expect("window duration is too large");

    let config = crate::load_config();

    let http_pool = reqwest::Client::new();
    let sw_client =
        crate::api::splitwise::Client::new(http_pool.clone(), config.splitwise.api_key.clone());
    let lm_client =
        crate::api::lunch_money::Client::new(http_pool, config.lunch_money.api_key.clone());

    let (start_window_str, end_window_str) =
        super::calculate_window_bounds(sync_args.from, window_duration);

    let dry_run_suffix = if sync_args.dry_run {
        format!(" {STYLE_WARNING}[DRY RUN]{STYLE_WARNING:#}")
    } else {
        "".to_string()
    };
    println! {};
    println! { "{STYLE_HEADER}⚡ Splitwise to Lunch Money Sync{}{STYLE_HEADER:#}", dry_run_suffix };
    println! { "{STYLE_DIM}──────────────────────────────────────────────────{STYLE_DIM:#}" };
    println! { "{STYLE_INFO}📅 Sync window boundary:{STYLE_INFO:#} {} to {}", start_window_str, end_window_str };
    println! {};

    // Fetch dependencies
    println! { "  {STYLE_DIM}Fetching Splitwise groups and expenses...{STYLE_DIM:#}" };
    let groups_res: GroupResponse = sw_client.fetch("get_groups", &[] as &[(&str, &str)]).await;
    let group_map: HashMap<u64, String> = groups_res
        .groups
        .into_iter()
        .map(|g| (g.id, g.name))
        .collect();

    let mut sw_query = vec![("dated_after", start_window_str.as_str()), ("limit", "0")];
    let dated_before_str;
    if sync_args.from.is_some() {
        dated_before_str = format!("{}T23:59:59Z", end_window_str);
        sw_query.push(("dated_before", dated_before_str.as_str()));
    }
    let expenses_res: ExpensesResponse = sw_client.fetch("get_expenses", &sw_query).await;

    let expenses = expenses_res.expenses;

    let mut sw_expense_categories = HashMap::new();
    for expense in &expenses {
        let ext_id = crate::api::ExternalId::Splitwise(expense.id);
        let cat_info = expense.category.as_ref().map(|c| (c.id, c.name.clone()));
        sw_expense_categories.insert(ext_id, cat_info);
    }

    // Verify configured manual accounts exist in Lunch Money
    let accounts_res: ManualAccountsResponse = lm_client
        .fetch("manual_accounts", &[] as &[(&str, &str)])
        .await;
    let target_accounts = crate::commands::resolve_target_accounts(
        &accounts_res,
        &config.lunch_money.custom_accounts,
    );
    verify_target_accounts(&target_accounts, &accounts_res);

    let (resolved_categories, lm_category_names) = resolve_categories(&lm_client, &config).await;

    let sw_category_id_to_path = fetch_splitwise_categories(&sw_client, &config).await;

    let lm_transactions = fetch_lunch_money_transactions(
        &lm_client,
        &target_accounts,
        &accounts_res,
        &start_window_str,
        &end_window_str,
    )
    .await;

    println! { "  {STYLE_DIM}Comparing transactions...{STYLE_DIM:#}" };
    println! {};

    let mut lm_tx_categories = HashMap::new();
    for t in &lm_transactions {
        lm_tx_categories.insert(t.id, (t.external_id.clone(), t.category_id));
    }

    let mut lm_map: HashMap<crate::api::ExternalId, Transaction> = lm_transactions
        .into_iter()
        .filter_map(|t| t.external_id.clone().map(|ext_id| (ext_id, t)))
        .collect();

    let (inserts, updates, deletes) = diff_transactions(
        expenses,
        &config,
        &target_accounts,
        &group_map,
        &mut lm_map,
        &sw_category_id_to_path,
        &resolved_categories,
        None,
        None,
    );

    execute_sync_actions(
        &deletes,
        &updates,
        &inserts,
        sync_args.dry_run,
        &lm_client,
        &accounts_res,
        &target_accounts,
        &lm_category_names,
        &sw_expense_categories,
        &sw_category_id_to_path,
        &lm_tx_categories,
    )
    .await;
}

pub(crate) async fn run_sync_group(sync_args: crate::cli::SyncGroupArgs) {
    let config = crate::load_config();

    if config
        .splitwise
        .ignored_groups
        .contains(&sync_args.group_id)
        && !sync_args.bypass_ignore
    {
        eprintln! {};
        eprintln! { "{STYLE_WARNING}⚠️ Warning:{STYLE_WARNING:#} Group {} is marked as ignored in configuration.", sync_args.group_id };
        eprintln! { "To force synchronization for this group, use the --bypass-ignore flag." };
        eprintln! {};
        std::process::exit(1);
    }

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
    println! {};
    println! { "{STYLE_HEADER}⚡ Splitwise to Lunch Money Sync Group{}{STYLE_HEADER:#}", dry_run_suffix };
    println! { "{STYLE_DIM}──────────────────────────────────────────────────{STYLE_DIM:#}" };

    // Fetch dependencies
    println! { "  {STYLE_DIM}Fetching Splitwise groups and expenses...{STYLE_DIM:#}" };
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

    println! { "{STYLE_INFO}👥 Group:{STYLE_INFO:#} {} (ID: {})", group_name, sync_args.group_id };
    if let Some(g) = target_group {
        let balance_str = super::format_group_balances(g, config.splitwise.user_id);
        println! { "{STYLE_INFO}💰 Balance:{STYLE_INFO:#} {}", balance_str };
    }
    println! {};

    let group_id_str = sync_args.group_id.to_string();
    let sw_query = [("group_id", group_id_str.as_str()), ("limit", "0")];
    let expenses_res: ExpensesResponse = sw_client.fetch("get_expenses", &sw_query).await;

    let mut sw_expense_categories = HashMap::new();
    for expense in &expenses_res.expenses {
        let ext_id = crate::api::ExternalId::Splitwise(expense.id);
        let cat_info = expense.category.as_ref().map(|c| (c.id, c.name.clone()));
        sw_expense_categories.insert(ext_id, cat_info);
    }

    // Verify configured manual accounts exist in Lunch Money
    let accounts_res: ManualAccountsResponse = lm_client
        .fetch("manual_accounts", &[] as &[(&str, &str)])
        .await;
    let target_accounts = crate::commands::resolve_target_accounts(
        &accounts_res,
        &config.lunch_money.custom_accounts,
    );
    verify_target_accounts(&target_accounts, &accounts_res);

    let (resolved_categories, lm_category_names) = resolve_categories(&lm_client, &config).await;

    let sw_category_id_to_path = fetch_splitwise_categories(&sw_client, &config).await;

    let mut tag_id = None;
    if let Some(ref tag_name) = sync_args.tag {
        println! { "  {STYLE_DIM}Resolving Lunch Money tag '{}'...{STYLE_DIM:#}", tag_name };
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
                println! { "   {STYLE_WARNING}Would create tag:{STYLE_WARNING:#} '{}'", tag_name };
                tag_id = Some(0);
            } else {
                println! { "  {STYLE_DIM}Creating new tag '{}'...{STYLE_DIM:#}", tag_name };
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

    let end_window_str = jiff::Timestamp::now()
        .to_zoned(jiff::tz::TimeZone::UTC)
        .strftime("%Y-%m-%d")
        .to_string();

    let lm_transactions = fetch_lunch_money_transactions(
        &lm_client,
        &target_accounts,
        &accounts_res,
        "2000-01-01",
        &end_window_str,
    )
    .await;

    println! { "  {STYLE_DIM}Comparing transactions...{STYLE_DIM:#}" };
    println! {};

    let mut lm_tx_categories = HashMap::new();
    for t in &lm_transactions {
        lm_tx_categories.insert(t.id, (t.external_id.clone(), t.category_id));
    }

    let mut lm_map: HashMap<crate::api::ExternalId, Transaction> = lm_transactions
        .into_iter()
        .filter_map(|t| t.external_id.clone().map(|ext_id| (ext_id, t)))
        .collect();

    let (inserts, updates, mut deletes) = diff_transactions(
        expenses_res.expenses,
        &config,
        &target_accounts,
        &group_map,
        &mut lm_map,
        &sw_category_id_to_path,
        &resolved_categories,
        Some(sync_args.group_id),
        tag_id.map(|id| vec![id]),
    );

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

    execute_sync_actions(
        &deletes,
        &updates,
        &inserts,
        sync_args.dry_run,
        &lm_client,
        &accounts_res,
        &target_accounts,
        &lm_category_names,
        &sw_expense_categories,
        &sw_category_id_to_path,
        &lm_tx_categories,
    )
    .await;
}

fn verify_target_accounts(
    target_accounts: &HashMap<crate::api::Currency, u64>,
    accounts_res: &ManualAccountsResponse,
) {
    if target_accounts.is_empty() {
        eprintln! {};
        eprintln! { "{STYLE_ERROR}❌ Error:{STYLE_ERROR:#} No active manual accounts found. Please set up an active 'Splitwise <CURRENCY>' manual account (e.g. 'Splitwise USD') in Lunch Money or configure [lunch_money.custom_accounts]." };
        eprintln! {};
        std::process::exit(1);
    }

    for (currency, &account_id) in target_accounts {
        if !accounts_res
            .manual_accounts
            .iter()
            .any(|acc| acc.id == account_id)
        {
            eprintln! {};
            eprintln! { "{STYLE_ERROR}❌ Error:{STYLE_ERROR:#} Configured manual account ID {} for currency '{}' has been deleted or does not exist in Lunch Money.", account_id, currency };
            eprintln! {};
            std::process::exit(1);
        }
    }
}

async fn fetch_splitwise_categories(
    sw_client: &crate::api::splitwise::Client,
    config: &crate::config::Config,
) -> HashMap<u32, String> {
    let mut sw_category_id_to_path = HashMap::new();
    if !config.categories.is_empty() {
        println! { "  {STYLE_DIM}Fetching Splitwise categories...{STYLE_DIM:#}" };
        let sw_categories_res: crate::api::splitwise::schema::CategoriesResponse = sw_client
            .fetch("get_categories", &[] as &[(&str, &str)])
            .await;
        for parent in sw_categories_res.categories {
            sw_category_id_to_path.insert(parent.id, parent.name.clone());
            for sub in parent.subcategories {
                let path = format!("{}:{}", parent.name, sub.name);
                sw_category_id_to_path.insert(sub.id, path);
            }
        }
    }
    sw_category_id_to_path
}

async fn fetch_lunch_money_transactions(
    lm_client: &crate::api::lunch_money::Client,
    target_accounts: &HashMap<crate::api::Currency, u64>,
    accounts_res: &ManualAccountsResponse,
    start_date_str: &str,
    end_date_str: &str,
) -> Vec<Transaction> {
    println! { "  {STYLE_DIM}Fetching Lunch Money transactions...{STYLE_DIM:#}" };
    let mut lm_transactions = Vec::new();
    for &account_id in target_accounts.values() {
        let account_id_str = account_id.to_string();
        let lm_query = [
            ("start_date", start_date_str),
            ("end_date", end_date_str),
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
    lm_transactions
}

fn diff_transactions(
    expenses: Vec<crate::api::splitwise::schema::Expense>,
    config: &crate::config::Config,
    target_accounts: &HashMap<crate::api::Currency, u64>,
    group_map: &HashMap<u64, String>,
    lm_map: &mut HashMap<crate::api::ExternalId, Transaction>,
    sw_category_id_to_path: &HashMap<u32, String>,
    resolved_categories: &HashMap<String, u64>,
    ignored_groups_exclude: Option<u64>,
    tag_ids: Option<Vec<u64>>,
) -> (Vec<InsertObject>, Vec<UpdateObject>, Vec<Transaction>) {
    let mut inserts = Vec::new();
    let mut updates = Vec::new();
    let mut deletes = Vec::new();

    for expense in expenses {
        let external_id = crate::api::ExternalId::Splitwise(expense.id);

        let net_balance = expense
            .users
            .iter()
            .find(|u| u.user_id == config.splitwise.user_id)
            .map(|u| u.net_balance)
            .unwrap_or(Decimal::ZERO);

        let is_ignored = expense.group_id.is_some_and(|gid| {
            config.splitwise.ignored_groups.contains(&gid) && Some(gid) != ignored_groups_exclude
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
            eprintln! {};
            eprintln! { "{STYLE_ERROR}❌ Error:{STYLE_ERROR:#} No manual account configured for currency '{}'.", expense.currency_code };
            eprintln! { "Please set up an active 'Splitwise {}' manual account in Lunch Money or configure [lunch_money.custom_accounts].", expense.currency_code };
            eprintln! {};
            std::process::exit(1);
        }

        let date_civil = expense.date.to_zoned(jiff::tz::TimeZone::UTC).date();

        let payee_str = format!(
            "Splitwise - {}",
            super::resolve_splitwise_payee(&expense, config.splitwise.user_id, group_map)
        );

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
            if let Some(ref cat) = expense.category {
                let path = sw_category_id_to_path.get(&cat.id);
                category_id = path
                    .and_then(|p| resolved_categories.get(p))
                    .or_else(|| resolved_categories.get(&cat.name))
                    .or_else(|| resolved_categories.get(&cat.id.to_string()))
                    .copied();
            }
            inserts.push(InsertObject {
                date: date_civil,
                amount: net_balance,
                currency: expense.currency_code.clone(),
                payee: payee_str,
                notes: expense.description,
                external_id,
                manual_account_id,
                status: crate::api::lunch_money::schema::TransactionStatus::Unreviewed,
                tag_ids: tag_ids.clone(),
                category_id,
            });
        }
    }

    (inserts, updates, deletes)
}

async fn execute_sync_actions(
    deletes: &[Transaction],
    updates: &[UpdateObject],
    inserts: &[InsertObject],
    dry_run: bool,
    lm_client: &crate::api::lunch_money::Client,
    accounts_res: &ManualAccountsResponse,
    target_accounts: &HashMap<crate::api::Currency, u64>,
    lm_category_names: &HashMap<u64, String>,
    sw_expense_categories: &HashMap<crate::api::ExternalId, Option<(u32, String)>>,
    sw_category_id_to_path: &HashMap<u32, String>,
    lm_tx_categories: &HashMap<u64, (Option<crate::api::ExternalId>, Option<u64>)>,
) {
    // Calculate dynamic category and payee column widths
    let mut payee_width = 0;
    let mut sw_cat_width = 0;
    let mut lm_cat_width = 0;
    {
        let get_clean_payee_len = |payee: &str| {
            let mut clean = payee;
            if clean.starts_with("Splitwise - ") {
                clean = &clean["Splitwise - ".len()..];
            }
            let len = clean.chars().count();
            if len > 50 { 50 } else { len }
        };

        let mut check_width = |payee: &str, sw_cat: Option<&str>, lm_cat: Option<&str>| {
            let p_len = get_clean_payee_len(payee);
            if p_len > payee_width {
                payee_width = p_len;
            }

            let sw_clean = match sw_cat {
                Some("Uncategorized:General") => None,
                other => other,
            };
            if let Some(sw) = sw_clean {
                if sw.len() > sw_cat_width {
                    sw_cat_width = sw.len();
                }
            }
            let lm_part = lm_cat.unwrap_or("?");
            if lm_part.len() > lm_cat_width {
                lm_cat_width = lm_part.len();
            }
        };

        for t in deletes {
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
            check_width(&t.payee, sw_category_name, category_name.as_deref());
        }

        for u in updates {
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
            check_width(&u.payee, sw_category_name, category_name.as_deref());
        }

        for ins in inserts {
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
            check_width(&ins.payee, sw_category_name, category_name.as_deref());
        }
    }

    // Execute batches
    if !deletes.is_empty() {
        println! { "🗑️  {STYLE_WARNING}Deleting {STYLE_WARNING:#}{} old/modified transaction(s) from Lunch Money:", deletes.len() };
        for t in deletes {
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
            println! { "   {STYLE_ERROR}-{STYLE_ERROR:#} {}", format_transaction_summary(&t.payee, t.amount, &t.currency, t.date, t.notes.as_deref().unwrap_or(""), sw_category_name, category_name.as_deref(), payee_width, sw_cat_width, lm_cat_width) };
        }
        println! {};

        if !dry_run {
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
        println! { "✎  {STYLE_INFO}Updating {STYLE_INFO:#}{} modified transaction(s) in Lunch Money:", updates.len() };
        for u in updates {
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
            println! { "   {STYLE_INFO}~{STYLE_INFO:#} {}", format_transaction_summary(&u.payee, u.amount, &u.currency, u.date, &u.notes, sw_category_name, category_name.as_deref(), payee_width, sw_cat_width, lm_cat_width) };
        }
        println! {};

        if !dry_run {
            for chunk in updates.chunks(500) {
                let mut chunk_txs = chunk.to_vec();
                for u in &mut chunk_txs {
                    let is_loan = accounts_res
                        .manual_accounts
                        .iter()
                        .find(|acc| target_accounts.get(&u.currency).copied() == Some(acc.id))
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
        println! { "✓  {STYLE_SUCCESS}Inserting {STYLE_SUCCESS:#}{} new transaction(s) to Lunch Money:", inserts.len() };
        for ins in inserts {
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
            println! { "   {STYLE_SUCCESS}+{STYLE_SUCCESS:#} {}", format_transaction_summary(&ins.payee, ins.amount, &ins.currency, ins.date, &ins.notes, sw_category_name, category_name.as_deref(), payee_width, sw_cat_width, lm_cat_width) };
        }
        println! {};

        if !dry_run {
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
        println! { "{STYLE_SUCCESS}✨ No changes detected. Lunch Money manual account is up-to-date!{STYLE_SUCCESS:#}" };
        println! {};
    } else if dry_run {
        println! { "{STYLE_WARNING}⚠️ Dry run complete! No changes were made to Lunch Money.{STYLE_WARNING:#}" };
        println! {};
    } else {
        println! { "{STYLE_SUCCESS}✨ Synchronization cycle complete!{STYLE_SUCCESS:#}" };
        println! {};
    }
}
