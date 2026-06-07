use crate::api::splitwise::schema::ExpensesResponse;
use crate::api::splitwise::schema::GroupResponse;
use crate::style::*;
use anstream::println;
use anyhow::Context;
use rust_decimal::Decimal;
use std::collections::HashMap;
use tabled::Table;
use tabled::Tabled;
use tabled::settings::Style;

#[derive(Tabled)]
struct ExpenseRecord {
    #[tabled(rename = "Date")]
    date: String,
    #[tabled(rename = "Group/Person")]
    payee: String,
    #[tabled(rename = "Description")]
    description: String,
    #[tabled(rename = "Net Balance")]
    net_balance: String,
}

fn print_expenses_table(
    expenses: Vec<crate::api::splitwise::schema::Expense>,
    config: &crate::config::Config,
    group_map: &HashMap<u64, String>,
) {
    let mut has_uninvolved = false;

    // Scan expenses to compute the maximum width of the numeric and currency sub-components.
    // This allows us to manually pad them for proper decimal/currency alignment.
    let (max_num_len, max_currency_len) =
        super::compute_max_widths(expenses.iter().map(|expense| {
            let net_balance = expense
                .users
                .iter()
                .find(|u| u.user_id == config.splitwise.user_id)
                .map(|u| u.net_balance)
                .unwrap_or(Decimal::ZERO);
            (net_balance, &expense.currency_code)
        }));

    let mut records = Vec::new();
    for expense in expenses {
        let net_balance = expense
            .users
            .iter()
            .find(|u| u.user_id == config.splitwise.user_id)
            .map(|u| u.net_balance)
            .unwrap_or(Decimal::ZERO);

        let date_str = expense
            .date
            .to_zoned(jiff::tz::TimeZone::UTC)
            .date()
            .strftime("%Y-%m-%d")
            .to_string();

        let payee_str =
            super::resolve_splitwise_payee(&expense, config.splitwise.user_id, group_map);

        let mut clean_payee = payee_str;
        if clean_payee.chars().count() > 30 {
            clean_payee = clean_payee.chars().take(27).collect::<String>();
            clean_payee.push_str("...");
        }

        let is_ignored = expense.group_id.is_some_and(|gid| {
            let name = group_map.get(&gid).map(|s| s.as_str());
            config.splitwise.is_group_ignored(gid, name)
        });

        // Styling and status tag
        let (style, status_tag, is_uninvolved) = if expense.deleted_at.is_some() {
            (STYLE_DIM, " [DELETED]", false)
        } else if is_ignored {
            (STYLE_WARNING, " [IGNORED]", false)
        } else if net_balance.is_zero() {
            has_uninvolved = true;
            (STYLE_DIM, "", true)
        } else if net_balance.is_sign_negative() {
            (STYLE_ERROR, "", false)
        } else {
            (STYLE_SUCCESS, "", false)
        };

        // Determine max allowed length for description
        let max_desc_len = 30_usize.saturating_sub(status_tag.len());
        let mut clean_desc = expense.description.trim().to_string();
        if clean_desc.chars().count() > max_desc_len {
            let truncate_to = max_desc_len.saturating_sub(3);
            clean_desc = clean_desc.chars().take(truncate_to).collect::<String>();
            clean_desc = format!("{}...", clean_desc.trim_end());
        }

        let desc_colored = if !status_tag.is_empty() {
            format!("{}{STYLE_DIM}{status_tag}{STYLE_DIM:#}", clean_desc)
        } else {
            clean_desc
        };

        // Format and align the balance column using our shared helper
        let balance_plain = super::format_aligned_balance(
            net_balance,
            &expense.currency_code,
            max_num_len,
            max_currency_len,
            is_uninvolved,
        );
        let balance_colored = format!("{}{}{:#}", style, balance_plain, style);

        records.push(ExpenseRecord {
            date: date_str,
            payee: clean_payee,
            description: desc_colored,
            net_balance: balance_colored,
        });
    }

    let mut table = Table::new(records);
    table.with(Style::rounded());
    println!("{}", table);

    if has_uninvolved {
        println! { "  {STYLE_DIM}* = uninvolved transaction (net balance is zero){STYLE_DIM:#}" };
        println! {};
    } else {
        println! {};
    }
}

pub(crate) async fn run_query_splitwise_window(
    args: crate::cli::QuerySplitwiseWindowArgs,
) -> anyhow::Result<()> {
    let window_duration =
        jiff::SignedDuration::try_from(args.window).context("window duration is too large")?;

    let config = crate::load_config()?;

    let http_pool = reqwest::Client::new();
    let sw_client =
        crate::api::splitwise::Client::new(http_pool.clone(), config.splitwise.api_key.clone());

    let (start_window_str, end_window_str) =
        super::calculate_window_bounds(args.from, window_duration);

    let bar = "─".repeat(92);

    println! {};
    println! { "{STYLE_HEADER}🔍 Querying Splitwise Expenses{STYLE_HEADER:#}" };
    println! { "{STYLE_DIM}{bar}{STYLE_DIM:#}" };
    println! { "{STYLE_INFO}📅 Window boundary:{STYLE_INFO:#} {} to {}", start_window_str, end_window_str };
    if args.no_groups {
        println! { "{STYLE_INFO}🚫 Filter:{STYLE_INFO:#} Non-group expenses only" };
    }
    println! {};

    println! { "  {STYLE_DIM}Fetching Splitwise groups and expenses...{STYLE_DIM:#}" };
    let groups_res: GroupResponse = sw_client
        .fetch("get_groups", &[] as &[(&str, &str)])
        .await?;
    let group_map: HashMap<u64, String> = groups_res
        .groups
        .into_iter()
        .map(|g| (g.id, g.name))
        .collect();

    let mut sw_query = vec![("dated_after", start_window_str.as_str()), ("limit", "0")];
    let dated_before_str;
    if args.from.is_some() {
        dated_before_str = format!("{}T23:59:59Z", end_window_str);
        sw_query.push(("dated_before", dated_before_str.as_str()));
    }
    let expenses_res: ExpensesResponse = sw_client.fetch("get_expenses", &sw_query).await?;

    let mut expenses = expenses_res.expenses;
    if args.no_groups {
        expenses.retain(|e| e.group_id.is_none());
    }

    if expenses.is_empty() {
        println! { "{STYLE_SUCCESS}✨ No expenses found in this window.{STYLE_SUCCESS:#}" };
        println! {};
        return Ok(());
    }

    print_expenses_table(expenses, &config, &group_map);
    Ok(())
}

pub(crate) async fn run_query_splitwise_group(
    args: crate::cli::QuerySplitwiseGroupArgs,
) -> anyhow::Result<()> {
    let config = crate::load_config()?;

    let http_pool = reqwest::Client::new();
    let sw_client =
        crate::api::splitwise::Client::new(http_pool.clone(), config.splitwise.api_key.clone());

    let bar = "─".repeat(92);

    println! {};
    println! { "{STYLE_HEADER}🔍 Querying Splitwise Group Expenses{STYLE_HEADER:#}" };
    println! { "{STYLE_DIM}{bar}{STYLE_DIM:#}" };

    println! { "  {STYLE_DIM}Fetching Splitwise groups and expenses...{STYLE_DIM:#}" };
    let groups_res: GroupResponse = sw_client
        .fetch("get_groups", &[] as &[(&str, &str)])
        .await?;
    let group_map: HashMap<u64, String> = groups_res
        .groups
        .iter()
        .map(|g| (g.id, g.name.clone()))
        .collect();

    let target_group = super::resolve_group(&groups_res.groups, &args.group)?;

    println! { "{STYLE_INFO}👥 Group:{STYLE_INFO:#} {} (ID: {})", target_group.name, target_group.id };
    if target_group.id != 0 {
        let balance_str = super::format_group_balances(&target_group, config.splitwise.user_id);
        println! { "{STYLE_INFO}💰 Balance:{STYLE_INFO:#} {}", balance_str };
    }
    println! {};

    let group_id_str = target_group.id.to_string();
    let sw_query = [("group_id", group_id_str.as_str()), ("limit", "0")];
    let expenses_res: ExpensesResponse = sw_client.fetch("get_expenses", &sw_query).await?;

    if expenses_res.expenses.is_empty() {
        println! { "{STYLE_SUCCESS}✨ No expenses found for this group.{STYLE_SUCCESS:#}" };
        println! {};
        return Ok(());
    }

    print_expenses_table(expenses_res.expenses, &config, &group_map);
    Ok(())
}

#[derive(Tabled)]
struct GroupRecord {
    #[tabled(rename = "Last Updated")]
    last_updated: String,
    #[tabled(rename = "Group ID")]
    group_id: u64,
    #[tabled(rename = "Group Name")]
    group_name: String,
    #[tabled(rename = "Balance")]
    balance: String,
}

pub(crate) async fn run_query_splitwise_groups() -> anyhow::Result<()> {
    let config = crate::load_config()?;

    let http_pool = reqwest::Client::new();
    let sw_client =
        crate::api::splitwise::Client::new(http_pool.clone(), config.splitwise.api_key.clone());

    println! {};
    println! { "{STYLE_HEADER}🔍 Querying Splitwise Groups{STYLE_HEADER:#}" };
    println! { "{STYLE_DIM}──────────────────────────────────────────────────{STYLE_DIM:#}" };

    let groups_res: GroupResponse = sw_client
        .fetch("get_groups", &[] as &[(&str, &str)])
        .await?;

    if groups_res.groups.is_empty() {
        println! { "{STYLE_WARNING}No groups found.{STYLE_WARNING:#}" };
        println! {};
        return Ok(());
    }

    let mut groups = groups_res.groups;
    groups.sort_by_key(|b| std::cmp::Reverse(b.updated_at));

    let mut records = Vec::new();
    for g in groups {
        let mut clean_name = g.name.clone();
        if clean_name.chars().count() > 40 {
            clean_name = clean_name.chars().take(37).collect::<String>();
            clean_name.push_str("...");
        }
        let date_str = g
            .updated_at
            .to_zoned(jiff::tz::TimeZone::UTC)
            .date()
            .strftime("%Y-%m-%d")
            .to_string();
        let balance_str = super::format_group_balances(&g, config.splitwise.user_id);
        records.push(GroupRecord {
            last_updated: date_str,
            group_id: g.id,
            group_name: clean_name,
            balance: balance_str,
        });
    }

    let mut table = Table::new(records);
    table.with(Style::rounded());
    println!("{}", table);
    println! {};
    Ok(())
}

pub(crate) async fn run_query_lunchmoney_categories() -> anyhow::Result<()> {
    let config = crate::load_config()?;

    let http_pool = reqwest::Client::new();
    let lm_client =
        crate::api::lunch_money::Client::new(http_pool, config.lunch_money.api_key.clone());

    let bar = "─".repeat(80);

    println! {};
    println! { "{STYLE_HEADER}🔍 Querying Lunch Money Categories{STYLE_HEADER:#}" };
    println! { "{STYLE_DIM}{bar}{STYLE_DIM:#}" };

    let categories_res: crate::api::lunch_money::schema::CategoriesResponse = lm_client
        .fetch("categories", &[("format", "nested")] as &[(&str, &str)])
        .await?;

    let categories: Vec<_> = categories_res.categories;

    if categories.is_empty() {
        println! { "{STYLE_WARNING}No categories found.{STYLE_WARNING:#}" };
        println! {};
        return Ok(());
    }

    println! { "  {:<10} {}", "ID", "Category Name" };
    println! { "  {STYLE_DIM}{bar}{STYLE_DIM:#}" };

    let mut has_archived = false;

    for cat in categories {
        let id_bracket = format!("[{}]", cat.id);
        let mut display_name = cat.name.clone();
        if cat.archived {
            has_archived = true;
            display_name.push_str(" *");
            println! { "  {STYLE_DIM}{:<10} {}{STYLE_DIM:#}", id_bracket, display_name };
        } else {
            println! { "  {:<10} {}", id_bracket, display_name };
        }

        if cat.is_group {
            if let Some(children) = cat.children {
                let count = children.len();
                for (idx, child) in children.into_iter().enumerate() {
                    let branch = if idx == count - 1 {
                        "└──"
                    } else {
                        "├──"
                    };
                    let child_id_bracket = format!("[{}]", child.id);
                    let mut child_display_name = child.name.clone();
                    if child.archived {
                        has_archived = true;
                        child_display_name.push_str(" *");
                        println! { "  {STYLE_DIM}{} {:<9} {}{STYLE_DIM:#}", branch, child_id_bracket, child_display_name };
                    } else {
                        println! { "  {} {:<9} {}", branch, child_id_bracket, child_display_name };
                    }
                }
            }
        }
    }
    println! {};

    if has_archived {
        println! { "  {STYLE_DIM}* denotes archived categories{STYLE_DIM:#}" };
        println! {};
    }
    Ok(())
}

#[derive(Tabled)]
struct TagRecord {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Tag Name")]
    tag_name: String,
}

pub(crate) async fn run_query_lunchmoney_tags() -> anyhow::Result<()> {
    let config = crate::load_config()?;

    let http_pool = reqwest::Client::new();
    let lm_client =
        crate::api::lunch_money::Client::new(http_pool, config.lunch_money.api_key.clone());

    println! {};
    println! { "{STYLE_HEADER}🔍 Querying Lunch Money Tags{STYLE_HEADER:#}" };
    println! { "{STYLE_DIM}──────────────────────────────────────────────────{STYLE_DIM:#}" };

    let tags_res: crate::api::lunch_money::schema::TagsResponse =
        lm_client.fetch("tags", &[] as &[(&str, &str)]).await?;

    let tags = tags_res.tags;

    if tags.is_empty() {
        println! { "{STYLE_WARNING}No tags found.{STYLE_WARNING:#}" };
        println! {};
        return Ok(());
    }

    let mut has_archived = false;
    let mut records = Vec::new();

    for tag in tags {
        let id_bracket = format!("[{}]", tag.id);
        let mut display_name = tag.name.clone();
        if tag.archived {
            has_archived = true;
            display_name.push_str(" *");
            records.push(TagRecord {
                id: format!("{}{}{:#}", STYLE_DIM, id_bracket, STYLE_DIM),
                tag_name: format!("{}{}{:#}", STYLE_DIM, display_name, STYLE_DIM),
            });
        } else {
            records.push(TagRecord {
                id: id_bracket,
                tag_name: display_name,
            });
        }
    }

    let mut table = Table::new(records);
    table.with(Style::rounded());
    println!("{}", table);
    println! {};

    if has_archived {
        println! { "  {STYLE_DIM}* denotes archived tags{STYLE_DIM:#}" };
        println! {};
    }
    Ok(())
}

pub(crate) async fn run_query_splitwise_categories() -> anyhow::Result<()> {
    let config = crate::load_config()?;

    let http_pool = reqwest::Client::new();
    let sw_client = crate::api::splitwise::Client::new(http_pool, config.splitwise.api_key.clone());

    let bar = "─".repeat(80);

    println! {};
    println! { "{STYLE_HEADER}🔍 Querying Splitwise Categories{STYLE_HEADER:#}" };
    println! { "{STYLE_DIM}{bar}{STYLE_DIM:#}" };

    let categories_res: crate::api::splitwise::schema::CategoriesResponse = sw_client
        .fetch("get_categories", &[] as &[(&str, &str)])
        .await?;

    let categories = categories_res.categories;

    if categories.is_empty() {
        println! { "{STYLE_WARNING}No categories found.{STYLE_WARNING:#}" };
        println! {};
        return Ok(());
    }

    println! { "  {:<10} {}", "ID", "Category Name" };
    println! { "  {STYLE_DIM}{bar}{STYLE_DIM:#}" };

    for cat in categories {
        let id_bracket = format!("[{}]", cat.id);
        println! { "  {:<10} {}", id_bracket, cat.name };

        let count = cat.subcategories.len();
        for (idx, subcat) in cat.subcategories.into_iter().enumerate() {
            let branch = if idx == count - 1 {
                "└──"
            } else {
                "├──"
            };
            let sub_id_bracket = format!("[{}]", subcat.id);
            println! { "  {} {:<9} {}", branch, sub_id_bracket, subcat.name };
        }
    }
    println! {};
    Ok(())
}

#[derive(Tabled)]
struct AccountRecord {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Type")]
    account_type: String,
    #[tabled(rename = "Balance")]
    balance: String,
    #[tabled(rename = "Status")]
    status: String,
    #[tabled(rename = "Mapped")]
    mapped: String,
}

pub(crate) async fn run_query_lunchmoney_accounts() -> anyhow::Result<()> {
    let config = crate::load_config()?;

    let http_pool = reqwest::Client::new();
    let lm_client =
        crate::api::lunch_money::Client::new(http_pool, config.lunch_money.api_key.clone());

    println! {};
    println! { "{STYLE_HEADER}🔍 Querying Lunch Money Manual Accounts{STYLE_HEADER:#}" };
    println! { "{STYLE_DIM}──────────────────────────────────────────────────{STYLE_DIM:#}" };

    let accounts_res: crate::api::lunch_money::schema::ManualAccountsResponse = lm_client
        .fetch("manual_accounts", &[] as &[(&str, &str)])
        .await?;

    let target_accounts: HashMap<u64, crate::api::Currency> =
        crate::commands::resolve_target_accounts(
            &accounts_res,
            &config.lunch_money.custom_accounts,
        )
        .into_iter()
        .map(|(currency, id)| (id, currency))
        .collect();

    let mut accounts = accounts_res.manual_accounts;

    if accounts.is_empty() {
        println! { "{STYLE_WARNING}No manual accounts found.{STYLE_WARNING:#}" };
        println! {};
        return Ok(());
    }

    // Sort accounts: active first, then by name
    accounts.sort_by(|a, b| match (a.status, b.status) {
        (
            crate::api::lunch_money::schema::AccountStatus::Active,
            crate::api::lunch_money::schema::AccountStatus::Closed,
        ) => std::cmp::Ordering::Less,
        (
            crate::api::lunch_money::schema::AccountStatus::Closed,
            crate::api::lunch_money::schema::AccountStatus::Active,
        ) => std::cmp::Ordering::Greater,
        _ => a.name.cmp(&b.name),
    });

    // We calculate the max width of the balance and currency sub-components. By right-aligning
    // the numeric part and left-aligning the currency code, we ensure that decimals and
    // currency codes line up vertically across all rows, independent of negative signs.
    let (max_num_len, max_currency_len) =
        super::compute_max_widths(accounts.iter().map(|acc| (acc.balance, &acc.currency)));

    let mut records = Vec::new();
    for acc in accounts {
        let acc_name = acc.display_name.as_deref().unwrap_or(&acc.name);
        let mut clean_name = acc_name.to_string();
        if clean_name.chars().count() > 18 {
            clean_name = clean_name.chars().take(15).collect::<String>();
            clean_name.push_str("...");
        }

        let id_bracket = format!("[{}]", acc.id);
        let type_str = format!("{:?}", acc.account_type);

        let balance_plain = super::format_aligned_balance(
            acc.balance,
            &acc.currency,
            max_num_len,
            max_currency_len,
            false,
        );

        let mapped_str = if let Some(currency) = target_accounts.get(&acc.id) {
            currency.to_uppercase()
        } else {
            "—".to_string()
        };

        let is_closed = acc.status == crate::api::lunch_money::schema::AccountStatus::Closed;

        if is_closed {
            let mapped_display = if target_accounts.contains_key(&acc.id) {
                format!("{}{}{:#}", STYLE_DIM, mapped_str, STYLE_DIM)
            } else {
                format!("{}{}{:#}", STYLE_DIM, "—", STYLE_DIM)
            };
            records.push(AccountRecord {
                id: format!("{}{}{:#}", STYLE_DIM, id_bracket, STYLE_DIM),
                name: format!("{}{}{:#}", STYLE_DIM, clean_name, STYLE_DIM),
                account_type: format!("{}{}{:#}", STYLE_DIM, type_str, STYLE_DIM),
                balance: format!("{}{}{:#}", STYLE_DIM, balance_plain, STYLE_DIM),
                status: format!("{}{}{:#}", STYLE_DIM, acc.status, STYLE_DIM),
                mapped: mapped_display,
            });
        } else {
            let mapped_display = if target_accounts.contains_key(&acc.id) {
                format!("{}{}{:#}", STYLE_INFO, mapped_str, STYLE_INFO)
            } else {
                format!("{}{}{:#}", STYLE_DIM, "—", STYLE_DIM)
            };
            records.push(AccountRecord {
                id: id_bracket,
                name: clean_name,
                account_type: type_str,
                balance: balance_plain,
                status: acc.status.to_string(),
                mapped: mapped_display,
            });
        }
    }

    let mut table = Table::new(records);
    table.with(Style::rounded());
    println!("{}", table);
    println! {};
    Ok(())
}
