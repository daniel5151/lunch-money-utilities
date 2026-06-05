pub(crate) mod init;
pub(crate) mod query;
pub(crate) mod sync;
pub(crate) mod sync_balances;

use crate::style::*;
use std::collections::HashMap;

fn resolve_target_accounts(
    accounts_res: &crate::api::lunch_money::schema::ManualAccountsResponse,
    custom_accounts: &HashMap<crate::api::Currency, u64>,
) -> HashMap<crate::api::Currency, u64> {
    let mut resolved = HashMap::new();

    // 1. Start with inferred accounts from the actual manual accounts in Lunch Money
    for acc in &accounts_res.manual_accounts {
        if acc.status == crate::api::lunch_money::schema::AccountStatus::Active {
            for name in [&acc.name, acc.display_name.as_deref().unwrap_or("")] {
                if name.is_empty() {
                    continue;
                }
                let name_lower = name.to_ascii_lowercase();
                if let Some(suffix) = name_lower.strip_prefix("splitwise ") {
                    if suffix.len() == 3 && suffix.chars().all(|c| c.is_ascii_alphabetic()) {
                        let currency = crate::api::Currency::new(suffix);
                        resolved.insert(currency, acc.id);
                        break;
                    }
                }
            }
        }
    }

    // 2. Override with custom_accounts (takes precedence)
    for (curr, &id) in custom_accounts {
        resolved.insert(curr.clone(), id);
    }

    resolved
}

fn resolve_splitwise_payee(
    expense: &crate::api::splitwise::schema::Expense,
    user_id: u64,
    group_map: &HashMap<u64, String>,
) -> String {
    match expense.group_id {
        Some(gid) => group_map
            .get(&gid)
            .cloned()
            .unwrap_or_else(|| "Unknown Group".to_string()),
        None => expense
            .users
            .iter()
            .find(|u| u.user_id != user_id)
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
}

fn format_group_balances(group: &crate::api::splitwise::schema::Group, user_id: u64) -> String {
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

fn calculate_window_bounds(
    from_date: Option<jiff::civil::Date>,
    window_duration: jiff::SignedDuration,
) -> (String, String) {
    let end_date = from_date.unwrap_or_else(|| {
        jiff::Timestamp::now()
            .to_zoned(jiff::tz::TimeZone::UTC)
            .date()
    });

    let start_date = (end_date
        .at(23, 59, 59, 999_999_999)
        .to_zoned(jiff::tz::TimeZone::UTC)
        .expect("valid datetime")
        .timestamp()
        - window_duration)
        .to_zoned(jiff::tz::TimeZone::UTC)
        .date();

    (start_date.to_string(), end_date.to_string())
}

/// Computes the maximum width of the formatted numeric part and currency code
/// across a sequence of amounts and currencies.
pub(crate) fn compute_max_widths<'a, I>(items: I) -> (usize, usize)
where
    I: IntoIterator<Item = (rust_decimal::Decimal, &'a crate::api::Currency)>,
{
    let mut max_num_len = 0;
    let mut max_currency_len = 0;
    for (amount, currency) in items {
        let num_len = format!("{:.2}", amount).len();
        if num_len > max_num_len {
            max_num_len = num_len;
        }
        let currency_len = currency.as_str().len();
        if currency_len > max_currency_len {
            max_currency_len = currency_len;
        }
    }
    (max_num_len, max_currency_len)
}

/// Formats a decimal amount and a currency code into a padded string to align decimals and
/// currency codes vertically across table rows. The parameter `is_uninvolved` determines
/// whether to append a `*` suffix (for uninvolved Splitwise expenses) or a space character.
pub(crate) fn format_aligned_balance(
    amount: rust_decimal::Decimal,
    currency: &crate::api::Currency,
    max_num_len: usize,
    max_currency_len: usize,
    is_uninvolved: bool,
) -> String {
    let num_str = format!("{:.2}", amount);
    let padded_num = format!("{:>width$}", num_str, width = max_num_len);
    let padded_currency = format!(
        "{:<width$}",
        currency.to_uppercase(),
        width = max_currency_len
    );

    let currency_suffix = if is_uninvolved {
        format!("{}*", padded_currency)
    } else {
        format!("{} ", padded_currency)
    };

    format!("{} {}", padded_num, currency_suffix)
}
