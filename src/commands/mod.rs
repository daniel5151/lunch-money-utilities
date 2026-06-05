pub mod init;
pub mod query;
pub mod sync;
pub mod sync_balances;

pub fn resolve_target_accounts(
    accounts_res: &crate::api::lunch_money::schema::ManualAccountsResponse,
    custom_accounts: &std::collections::HashMap<String, u64>,
) -> std::collections::HashMap<String, u64> {
    let mut resolved = std::collections::HashMap::new();

    // 1. Start with inferred accounts from the actual manual accounts in Lunch Money
    for acc in &accounts_res.manual_accounts {
        if acc.status == "active" {
            for name in [&acc.name, acc.display_name.as_deref().unwrap_or("")] {
                if name.is_empty() {
                    continue;
                }
                let name_lower = name.to_ascii_lowercase();
                if name_lower.starts_with("splitwise ") {
                    let suffix = &name_lower[10..];
                    if suffix.len() == 3 && suffix.chars().all(|c| c.is_ascii_alphabetic()) {
                        let currency = suffix.to_ascii_uppercase();
                        resolved.insert(currency, acc.id);
                        break;
                    }
                }
            }
        }
    }

    // 2. Override with custom_accounts (takes precedence)
    for (curr, &id) in custom_accounts {
        resolved.insert(curr.to_uppercase(), id);
    }

    resolved
}
