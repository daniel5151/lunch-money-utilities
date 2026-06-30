use anstream::println;
use rust_decimal::Decimal;
use tabled::Table;
use tabled::Tabled;
use tabled::settings::Style;

use lm_common::style::*;

pub(crate) async fn run_query_categories(
    lm_client: &lunch_money::client::Client,
) -> anyhow::Result<()> {
    let bar = "─".repeat(80);

    println! {};
    println! { "{STYLE_HEADER}🔍 Querying Lunch Money Categories{STYLE_HEADER:#}" };
    println! { "{STYLE_DIM}{bar}{STYLE_DIM:#}" };

    let query = lunch_money::categories::query_params::CategoryQuery::builder()
        .format("nested".to_string())
        .build();
    let categories = lm_client.fetch_categories(&query).await?;

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

pub(crate) async fn run_query_tags(
    lm_client: &lunch_money::client::Client,
) -> anyhow::Result<()> {
    println! {};
    println! { "{STYLE_HEADER}🔍 Querying Lunch Money Tags{STYLE_HEADER:#}" };
    println! { "{STYLE_DIM}──────────────────────────────────────────────────{STYLE_DIM:#}" };

    let tags = lm_client.fetch_tags().await?;

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
}

pub(crate) async fn run_query_accounts(
    lm_client: &lunch_money::client::Client,
) -> anyhow::Result<()> {
    use lunch_money::manual_accounts::schemas::AccountStatus;

    println! {};
    println! { "{STYLE_HEADER}🔍 Querying Lunch Money Manual Accounts{STYLE_HEADER:#}" };
    println! { "{STYLE_DIM}──────────────────────────────────────────────────{STYLE_DIM:#}" };

    let mut accounts = lm_client.fetch_manual_accounts().await?;

    if accounts.is_empty() {
        println! { "{STYLE_WARNING}No manual accounts found.{STYLE_WARNING:#}" };
        println! {};
        return Ok(());
    }

    // Sort accounts: active first, then by name
    accounts.sort_by(|a, b| match (a.status, b.status) {
        (AccountStatus::Active, AccountStatus::Closed) => std::cmp::Ordering::Less,
        (AccountStatus::Closed, AccountStatus::Active) => std::cmp::Ordering::Greater,
        _ => a.name.cmp(&b.name),
    });

    // Compute alignment widths for balance column
    let mut max_num_len = 0;
    let mut max_currency_len = 0;
    for acc in &accounts {
        let num_len = format!("{:.2}", acc.balance).len();
        if num_len > max_num_len {
            max_num_len = num_len;
        }
        let currency_len = acc.currency.as_str().len();
        if currency_len > max_currency_len {
            max_currency_len = currency_len;
        }
    }

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

        let balance_plain = format_aligned_balance(
            acc.balance,
            &acc.currency,
            max_num_len,
            max_currency_len,
        );

        let is_closed = acc.status == AccountStatus::Closed;

        if is_closed {
            records.push(AccountRecord {
                id: format!("{}{}{:#}", STYLE_DIM, id_bracket, STYLE_DIM),
                name: format!("{}{}{:#}", STYLE_DIM, clean_name, STYLE_DIM),
                account_type: format!("{}{}{:#}", STYLE_DIM, type_str, STYLE_DIM),
                balance: format!("{}{}{:#}", STYLE_DIM, balance_plain, STYLE_DIM),
                status: format!("{}{}{:#}", STYLE_DIM, acc.status, STYLE_DIM),
            });
        } else {
            records.push(AccountRecord {
                id: id_bracket,
                name: clean_name,
                account_type: type_str,
                balance: balance_plain,
                status: acc.status.to_string(),
            });
        }
    }

    let mut table = Table::new(records);
    table.with(Style::rounded());
    println!("{}", table);
    println! {};
    Ok(())
}

fn format_aligned_balance(
    amount: Decimal,
    currency: &lunch_money::core::Currency,
    max_num_len: usize,
    max_currency_len: usize,
) -> String {
    let num_str = format!("{:.2}", amount);
    let padded_num = format!("{:>width$}", num_str, width = max_num_len);
    let padded_currency = format!(
        "{:<width$}",
        currency.to_uppercase(),
        width = max_currency_len
    );
    format!("{} {} ", padded_num, padded_currency)
}
