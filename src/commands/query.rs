use crate::api::splitwise::schema::ExpensesResponse;
use crate::api::splitwise::schema::GroupResponse;
use crate::style::*;
use anstream::println;
use rust_decimal::Decimal;
use std::collections::HashMap;

pub fn format_group_balances(group: &crate::api::splitwise::schema::Group, user_id: u64) -> String {
    let mut parts = Vec::new();
    if let Some(members) = &group.members {
        if let Some(member) = members.iter().find(|m| m.id == user_id) {
            for bal in &member.balance {
                let amount = bal.amount;
                let currency = &bal.currency_code;
                let amount_str = format!("{:.2} {}", amount, currency);
                let styled = if amount.is_sign_negative() {
                    format!(
                        "{}{}{}",
                        STYLE_ERROR,
                        amount_str,
                        STYLE_ERROR.render_reset()
                    )
                } else if amount.is_zero() {
                    format!("{}{}{}", STYLE_DIM, amount_str, STYLE_DIM.render_reset())
                } else {
                    format!(
                        "{}{}{}",
                        STYLE_SUCCESS,
                        amount_str,
                        STYLE_SUCCESS.render_reset()
                    )
                };
                parts.push(styled);
            }
        }
    }
    if parts.is_empty() {
        format!("{}—{}", STYLE_DIM, STYLE_DIM.render_reset())
    } else {
        parts.join(", ")
    }
}

fn print_expenses_table(
    expenses: Vec<crate::api::splitwise::schema::Expense>,
    config: &crate::config::Config,
    group_map: &HashMap<u64, String>,
) {
    let mut has_uninvolved = false;

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

        let payee_str = match expense.group_id {
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
        };

        let mut clean_payee = payee_str;
        if clean_payee.chars().count() > 30 {
            clean_payee = clean_payee.chars().take(27).collect::<String>();
            clean_payee.push_str("...");
        }

        let is_ignored = expense
            .group_id
            .is_some_and(|gid| config.splitwise.ignored_groups.contains(&gid));

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

        // Determine max allowed length for description, so description + status_tag is exactly 30 visible chars
        let max_desc_len = 30_usize.saturating_sub(status_tag.len());
        let mut clean_desc = expense.description.trim().to_string();
        if clean_desc.chars().count() > max_desc_len {
            let truncate_to = max_desc_len.saturating_sub(3);
            clean_desc = clean_desc.chars().take(truncate_to).collect::<String>();
            clean_desc = format!("{}...", clean_desc.trim_end());
        }

        let balance_plain = format!("{:>12}", net_balance);
        let balance_colored = format!("{}{}{:#}", style, balance_plain, style);

        let desc_colored = if !status_tag.is_empty() {
            let padding_spaces =
                " ".repeat(30_usize.saturating_sub(clean_desc.len() + status_tag.len()));
            format!(
                "{}{STYLE_DIM}{status_tag}{STYLE_DIM:#}{}",
                clean_desc, padding_spaces
            )
        } else {
            format!("{:<30}", clean_desc)
        };

        let currency_suffix = if is_uninvolved {
            format!("{}*", expense.currency_code.to_uppercase())
        } else {
            expense.currency_code.to_uppercase()
        };

        println! { "  {:<10}  {:<30}  {}  {} {}", date_str, clean_payee, desc_colored, balance_colored, currency_suffix };
    }

    if has_uninvolved {
        println! { "  {STYLE_DIM}* = uninvolved transaction (net balance is zero){STYLE_DIM:#}" };
        println! {};
    } else {
        println! {};
    }
}

pub async fn run_query_splitwise_window(args: crate::cli::QuerySplitwiseWindowArgs) {
    let window_duration =
        jiff::SignedDuration::try_from(args.window).expect("window duration is too large");

    let config = crate::load_config();

    let http_pool = reqwest::Client::new();
    let sw_client =
        crate::api::splitwise::Client::new(http_pool.clone(), config.splitwise.api_key.clone());

    let start_window = jiff::Timestamp::now() - window_duration;
    let start_window_str = start_window
        .to_zoned(jiff::tz::TimeZone::UTC)
        .strftime("%Y-%m-%d")
        .to_string();

    let bar = "─".repeat(92);

    println! {};
    println! { "{STYLE_HEADER}🔍 Querying Splitwise Expenses{STYLE_HEADER:#}" };
    println! { "{STYLE_DIM}{bar}{STYLE_DIM:#}" };
    println! { "{STYLE_INFO}📅 Window boundary:{STYLE_INFO:#} {}", start_window_str };
    println! {};

    println! { "  {STYLE_DIM}Fetching Splitwise groups and expenses...{STYLE_DIM:#}" };
    let groups_res: GroupResponse = sw_client.fetch("get_groups", &[] as &[(&str, &str)]).await;
    let group_map: HashMap<u64, String> = groups_res
        .groups
        .into_iter()
        .map(|g| (g.id, g.name))
        .collect();

    let sw_query = [("dated_after", start_window_str.as_str()), ("limit", "0")];
    let expenses_res: ExpensesResponse = sw_client.fetch("get_expenses", &sw_query).await;

    if expenses_res.expenses.is_empty() {
        println! { "{STYLE_SUCCESS}✨ No expenses found in this window.{STYLE_SUCCESS:#}" };
        println! {};
        return;
    }

    println! { "  {:<10}  {:<30}  {:<30}  {:>12}", "Date", "Group/Person", "Description", "Net Balance" };
    println! { "  {STYLE_DIM}{bar}{STYLE_DIM:#}" };

    print_expenses_table(expenses_res.expenses, &config, &group_map);
}

pub async fn run_query_splitwise_group(args: crate::cli::QuerySplitwiseGroupArgs) {
    let config = crate::load_config();

    let http_pool = reqwest::Client::new();
    let sw_client =
        crate::api::splitwise::Client::new(http_pool.clone(), config.splitwise.api_key.clone());

    let bar = "─".repeat(92);

    println! {};
    println! { "{STYLE_HEADER}🔍 Querying Splitwise Group Expenses{STYLE_HEADER:#}" };
    println! { "{STYLE_DIM}{bar}{STYLE_DIM:#}" };

    println! { "  {STYLE_DIM}Fetching Splitwise groups and expenses...{STYLE_DIM:#}" };
    let groups_res: GroupResponse = sw_client.fetch("get_groups", &[] as &[(&str, &str)]).await;
    let group_map: HashMap<u64, String> = groups_res
        .groups
        .iter()
        .map(|g| (g.id, g.name.clone()))
        .collect();

    let target_group = groups_res.groups.iter().find(|g| g.id == args.group_id);

    let group_name = target_group
        .map(|g| g.name.clone())
        .unwrap_or_else(|| "Unknown Group".to_string());

    println! { "{STYLE_INFO}👥 Group:{STYLE_INFO:#} {} (ID: {})", group_name, args.group_id };
    if let Some(g) = target_group {
        let balance_str = format_group_balances(g, config.splitwise.user_id);
        println! { "{STYLE_INFO}💰 Balance:{STYLE_INFO:#} {}", balance_str };
    }
    println! {};

    let group_id_str = args.group_id.to_string();
    let sw_query = [("group_id", group_id_str.as_str()), ("limit", "0")];
    let expenses_res: ExpensesResponse = sw_client.fetch("get_expenses", &sw_query).await;

    if expenses_res.expenses.is_empty() {
        println! { "{STYLE_SUCCESS}✨ No expenses found for this group.{STYLE_SUCCESS:#}" };
        println! {};
        return;
    }

    println! { "  {:<10}  {:<30}  {:<30}  {:>12}", "Date", "Group/Person", "Description", "Net Balance" };
    println! { "  {STYLE_DIM}{bar}{STYLE_DIM:#}" };

    print_expenses_table(expenses_res.expenses, &config, &group_map);
}

pub async fn run_query_splitwise_get_groups() {
    let config = crate::load_config();

    let http_pool = reqwest::Client::new();
    let sw_client =
        crate::api::splitwise::Client::new(http_pool.clone(), config.splitwise.api_key.clone());

    let bar = "─".repeat(110);

    println! {};
    println! { "{STYLE_HEADER}🔍 Querying Splitwise Groups{STYLE_HEADER:#}" };
    println! { "{STYLE_DIM}{bar}{STYLE_DIM:#}" };

    let groups_res: GroupResponse = sw_client.fetch("get_groups", &[] as &[(&str, &str)]).await;

    if groups_res.groups.is_empty() {
        println! { "{STYLE_WARNING}No groups found.{STYLE_WARNING:#}" };
        println! {};
        return;
    }

    println! { "  {:<15}  {:<15}  {:<40}  {}", "Last Updated", "Group ID", "Group Name", "Balance" };
    println! { "  {STYLE_DIM}{bar}{STYLE_DIM:#}" };

    let mut groups = groups_res.groups;
    groups.sort_by_key(|b| std::cmp::Reverse(b.updated_at));

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
        let balance_str = format_group_balances(&g, config.splitwise.user_id);
        println! { "  {:<15}  {:<15}  {:<40}  {}", date_str, g.id, clean_name, balance_str };
    }
    println! {};
}

pub async fn run_query_lunchmoney_categories() {
    let config = crate::load_config();

    let http_pool = reqwest::Client::new();
    let lm_client =
        crate::api::lunch_money::Client::new(http_pool, config.lunch_money.api_key.clone());

    let bar = "─".repeat(80);

    println! {};
    println! { "{STYLE_HEADER}🔍 Querying Lunch Money Categories{STYLE_HEADER:#}" };
    println! { "{STYLE_DIM}{bar}{STYLE_DIM:#}" };

    let categories_res: crate::api::lunch_money::schema::CategoriesResponse = lm_client
        .fetch("categories", &[("format", "nested")] as &[(&str, &str)])
        .await;

    let categories: Vec<_> = categories_res.categories;

    if categories.is_empty() {
        println! { "{STYLE_WARNING}No categories found.{STYLE_WARNING:#}" };
        println! {};
        return;
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
}

pub async fn run_query_lunchmoney_tags() {
    let config = crate::load_config();

    let http_pool = reqwest::Client::new();
    let lm_client =
        crate::api::lunch_money::Client::new(http_pool, config.lunch_money.api_key.clone());

    let bar = "─".repeat(80);

    println! {};
    println! { "{STYLE_HEADER}🔍 Querying Lunch Money Tags{STYLE_HEADER:#}" };
    println! { "{STYLE_DIM}{bar}{STYLE_DIM:#}" };

    let tags_res: crate::api::lunch_money::schema::TagsResponse =
        lm_client.fetch("tags", &[] as &[(&str, &str)]).await;

    let tags = tags_res.tags;

    if tags.is_empty() {
        println! { "{STYLE_WARNING}No tags found.{STYLE_WARNING:#}" };
        println! {};
        return;
    }

    println! { "  {:<10} {}", "ID", "Tag Name" };
    println! { "  {STYLE_DIM}{bar}{STYLE_DIM:#}" };

    let mut has_archived = false;

    for tag in tags {
        let id_bracket = format!("[{}]", tag.id);
        let mut display_name = tag.name.clone();
        if tag.archived {
            has_archived = true;
            display_name.push_str(" *");
            println! { "  {STYLE_DIM}{:<10} {}{STYLE_DIM:#}", id_bracket, display_name };
        } else {
            println! { "  {:<10} {}", id_bracket, display_name };
        }
    }
    println! {};

    if has_archived {
        println! { "  {STYLE_DIM}* denotes archived tags{STYLE_DIM:#}" };
        println! {};
    }
}

pub async fn run_query_splitwise_categories() {
    let config = crate::load_config();

    let http_pool = reqwest::Client::new();
    let sw_client = crate::api::splitwise::Client::new(http_pool, config.splitwise.api_key.clone());

    let bar = "─".repeat(80);

    println! {};
    println! { "{STYLE_HEADER}🔍 Querying Splitwise Categories{STYLE_HEADER:#}" };
    println! { "{STYLE_DIM}{bar}{STYLE_DIM:#}" };

    let categories_res: crate::api::splitwise::schema::CategoriesResponse = sw_client
        .fetch("get_categories", &[] as &[(&str, &str)])
        .await;

    let categories = categories_res.categories;

    if categories.is_empty() {
        println! { "{STYLE_WARNING}No categories found.{STYLE_WARNING:#}" };
        println! {};
        return;
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
}

pub async fn run_query_lunchmoney_accounts() {
    let config = crate::load_config();

    let http_pool = reqwest::Client::new();
    let lm_client =
        crate::api::lunch_money::Client::new(http_pool, config.lunch_money.api_key.clone());

    let bar = "─".repeat(80);

    println! {};
    println! { "{STYLE_HEADER}🔍 Querying Lunch Money Manual Accounts{STYLE_HEADER:#}" };
    println! { "{STYLE_DIM}{bar}{STYLE_DIM:#}" };

    let accounts_res: crate::api::lunch_money::schema::ManualAccountsResponse = lm_client
        .fetch("manual_accounts", &[] as &[(&str, &str)])
        .await;

    let mut accounts = accounts_res.manual_accounts;

    if accounts.is_empty() {
        println! { "{STYLE_WARNING}No manual accounts found.{STYLE_WARNING:#}" };
        println! {};
        return;
    }

    println! { "  {:<10}  {:<18}  {:<12}  {:>11}  {:<6}  {}", "ID", "Name", "Type", "Balance", "Status", "Mapped" };
    println! { "  {STYLE_DIM}{bar}{STYLE_DIM:#}" };

    let target_accounts: HashMap<u64, String> = config
        .lunch_money
        .target_accounts
        .iter()
        .map(|(currency, &id)| (id, currency.to_uppercase()))
        .collect();

    // Sort accounts: active first, then by name
    accounts.sort_by(|a, b| match (a.status.as_str(), b.status.as_str()) {
        ("active", "closed") => std::cmp::Ordering::Less,
        ("closed", "active") => std::cmp::Ordering::Greater,
        _ => a.name.cmp(&b.name),
    });

    for acc in accounts {
        let acc_name = acc.display_name.as_deref().unwrap_or(&acc.name);
        let mut clean_name = acc_name.to_string();
        if clean_name.chars().count() > 18 {
            clean_name = clean_name.chars().take(15).collect::<String>();
            clean_name.push_str("...");
        }

        let id_bracket = format!("[{}]", acc.id);
        let type_str = format!("{:?}", acc.account_type);
        let balance_str = format!("{:.2} {}", acc.balance, acc.currency.to_uppercase());

        let mapped_str = if let Some(currency) = target_accounts.get(&acc.id) {
            currency.to_uppercase()
        } else {
            "—".to_string()
        };

        let is_closed = acc.status == "closed";

        if is_closed {
            let mapped_display = if target_accounts.contains_key(&acc.id) {
                format!("{}{}{:#}", STYLE_DIM, mapped_str, STYLE_DIM)
            } else {
                format!("{}{}{:#}", STYLE_DIM, "—", STYLE_DIM)
            };
            println! { "  {STYLE_DIM}{:<10}  {:<18}  {:<12}  {:>11}  {:<6}  {}{STYLE_DIM:#}",
                id_bracket, clean_name, type_str, balance_str, acc.status, mapped_display
            };
        } else {
            let mapped_display = if target_accounts.contains_key(&acc.id) {
                format!("{}{}{:#}", STYLE_INFO, mapped_str, STYLE_INFO)
            } else {
                format!("{}{}{:#}", STYLE_DIM, "—", STYLE_DIM)
            };
            println! { "  {:<10}  {:<18}  {:<12}  {:>11}  {:<6}  {}",
                id_bracket, clean_name, type_str, balance_str, acc.status, mapped_display
            };
        }
    }
    println! {};
}
