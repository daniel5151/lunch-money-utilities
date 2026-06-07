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

pub(crate) fn resolve_group(
    groups: &[crate::api::splitwise::schema::Group],
    input: &str,
) -> Result<crate::api::splitwise::schema::Group, String> {
    let input_trimmed = input.trim();
    let parsed_id = input_trimmed.parse::<u64>().ok();

    // 1. Search for exact matches (by ID or exact name) in real groups
    let mut exact_matches = Vec::new();
    for g in groups {
        let matches_id = parsed_id == Some(g.id);
        let matches_name = g.name == input_trimmed;
        if matches_id || matches_name {
            exact_matches.push(g.clone());
        }
    }

    if exact_matches.len() > 1 {
        let mut msg = format!("Multiple groups found matching \"{}\":\n", input_trimmed);
        for g in &exact_matches {
            msg.push_str(&format!("  - ID: {} (Name: \"{}\")\n", g.id, g.name));
        }
        msg.push_str("Please specify the group by its ID to resolve ambiguity.");
        return Err(msg);
    }

    if exact_matches.len() == 1 {
        return Ok(exact_matches[0].clone());
    }

    // 2. Search for case-insensitive name matches in real groups
    let lower_input = input_trimmed.to_lowercase();
    let mut case_insensitive_matches = Vec::new();
    for g in groups {
        if g.name.to_lowercase() == lower_input {
            case_insensitive_matches.push(g.clone());
        }
    }

    if case_insensitive_matches.len() > 1 {
        let mut msg = format!(
            "Multiple groups found matching \"{}\" (case-insensitive):\n",
            input_trimmed
        );
        for g in &case_insensitive_matches {
            msg.push_str(&format!("  - ID: {} (Name: \"{}\")\n", g.id, g.name));
        }
        msg.push_str("Please specify the group by its ID to resolve ambiguity.");
        return Err(msg);
    }

    if case_insensitive_matches.len() == 1 {
        return Ok(case_insensitive_matches[0].clone());
    }

    // 3. Fallback to synthetic non-group if input is "0" or "non-group" (case-insensitive)
    if input_trimmed == "0" || lower_input == "non-group" {
        return Ok(crate::api::splitwise::schema::Group {
            id: 0,
            name: "Non-group".to_string(),
            updated_at: jiff::Timestamp::from_second(0).unwrap(), // dummy timestamp
            members: None,
        });
    }

    // 4. Not found
    Err(format!(
        "No Splitwise group found matching \"{}\".",
        input_trimmed
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::splitwise::schema::Group;
    use jiff::Timestamp;

    fn make_group(id: u64, name: &str) -> Group {
        Group {
            id,
            name: name.to_string(),
            updated_at: Timestamp::from_second(0).unwrap(),
            members: None,
        }
    }

    #[test]
    fn test_resolve_group_exact_id() {
        let groups = vec![make_group(12345, "Roommates"), make_group(67890, "Family")];

        let resolved = resolve_group(&groups, "12345").unwrap();
        assert_eq!(resolved.id, 12345);
        assert_eq!(resolved.name, "Roommates");
    }

    #[test]
    fn test_resolve_group_exact_name() {
        let groups = vec![make_group(12345, "Roommates"), make_group(67890, "Family")];

        let resolved = resolve_group(&groups, "Family").unwrap();
        assert_eq!(resolved.id, 67890);
        assert_eq!(resolved.name, "Family");
    }

    #[test]
    fn test_resolve_group_case_insensitive() {
        let groups = vec![make_group(12345, "Roommates"), make_group(67890, "Family")];

        let resolved = resolve_group(&groups, "family").unwrap();
        assert_eq!(resolved.id, 67890);
        assert_eq!(resolved.name, "Family");
    }

    #[test]
    fn test_resolve_group_ambiguity_exact() {
        let groups = vec![make_group(12345, "Roommates"), make_group(67890, "12345")];

        // "12345" matches ID of group 1, and name of group 2
        let err = resolve_group(&groups, "12345").unwrap_err();
        assert!(err.contains("Multiple groups found matching"));
    }

    #[test]
    fn test_resolve_group_ambiguity_case_insensitive() {
        let groups = vec![
            make_group(12345, "Roommates"),
            make_group(67890, "roommates"),
        ];

        // "Roommates" matches both exact and case-insensitive. But one is exact, so it is preferred!
        let resolved = resolve_group(&groups, "Roommates").unwrap();
        assert_eq!(resolved.id, 12345);

        // "ROOMMATES" is case-insensitive for both, so it is ambiguous
        let err = resolve_group(&groups, "ROOMMATES").unwrap_err();
        assert!(err.contains("Multiple groups found matching"));
    }

    #[test]
    fn test_resolve_group_synthetic_non_group() {
        let groups = vec![make_group(12345, "Roommates")];

        let resolved = resolve_group(&groups, "0").unwrap();
        assert_eq!(resolved.id, 0);
        assert_eq!(resolved.name, "Non-group");

        let resolved2 = resolve_group(&groups, "non-group").unwrap();
        assert_eq!(resolved2.id, 0);
        assert_eq!(resolved2.name, "Non-group");
    }

    #[test]
    fn test_resolve_group_synthetic_non_group_overridden() {
        let groups = vec![make_group(12345, "Roommates"), make_group(999, "Non-group")];

        // If there's a real group named "Non-group", it takes precedence
        let resolved = resolve_group(&groups, "non-group").unwrap();
        assert_eq!(resolved.id, 999);
        assert_eq!(resolved.name, "Non-group");
    }

    #[test]
    fn test_resolve_group_not_found() {
        let groups = vec![make_group(12345, "Roommates")];

        let err = resolve_group(&groups, "Unknown").unwrap_err();
        assert!(err.contains("No Splitwise group found matching"));
    }
}
