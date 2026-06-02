use crate::api::lunch_money::schema::ManualAccountsResponse;
use crate::{STYLE_DIM, STYLE_ERROR, STYLE_HEADER, STYLE_SUCCESS, STYLE_WARNING};
use reqwest::Method;
use rust_decimal::Decimal;
use std::collections::HashMap;

pub async fn run_sync_balances(args: crate::cli::SyncBalancesArgs) {
    let config = crate::load_config();

    let http_pool = reqwest::Client::new();
    let sw_client =
        crate::api::splitwise::Client::new(http_pool.clone(), config.splitwise.api_key.clone());
    let lm_client =
        crate::api::lunch_money::Client::new(http_pool.clone(), config.lunch_money.api_key.clone());

    anstream::println! {};
    anstream::println! { "{STYLE_HEADER}🔄 Syncing Splitwise Balances to Lunch Money{STYLE_HEADER:#}" };
    if args.dry_run {
        anstream::println! { "{STYLE_WARNING}⚠️  Running in DRY RUN mode. No changes will be made to Lunch Money.{STYLE_WARNING:#}" };
    }
    anstream::println! { "{STYLE_DIM}─────────────────────────────────────────────────────────────────{STYLE_DIM:#}" };

    anstream::println! { "  {STYLE_DIM}Fetching Splitwise friends...{STYLE_DIM:#}" };
    let friends_res: crate::api::splitwise::schema::FriendsResponse =
        sw_client.fetch("get_friends", &[] as &[(&str, &str)]).await;

    let mut global_balances: HashMap<String, Decimal> = HashMap::new();
    for friend in friends_res.friends {
        for bal in friend.balance {
            let currency = bal.currency_code.to_uppercase();
            *global_balances.entry(currency).or_insert(Decimal::ZERO) += bal.amount;
        }
    }

    anstream::println! { "  {STYLE_DIM}Fetching Lunch Money manual accounts...{STYLE_DIM:#}" };
    let accounts_res: ManualAccountsResponse = lm_client
        .fetch("manual_accounts", &[] as &[(&str, &str)])
        .await;

    // Normalize config keys to uppercase
    let target_accounts: HashMap<String, u64> = config
        .lunch_money
        .target_accounts
        .iter()
        .map(|(k, v)| (k.to_uppercase(), *v))
        .collect();

    let mut has_updates = false;

    for (currency, &account_id) in &target_accounts {
        let acc = match accounts_res
            .manual_accounts
            .iter()
            .find(|a| a.id == account_id)
        {
            Some(a) => a,
            None => {
                anstream::eprintln! {};
                anstream::eprintln! { "{STYLE_ERROR}❌ Error:{STYLE_ERROR:#} Configured manual account ID {} for currency '{}' has been deleted or does not exist in Lunch Money.", account_id, currency };
                std::process::exit(1);
            }
        };

        let splitwise_balance = global_balances
            .get(currency)
            .copied()
            .unwrap_or(Decimal::ZERO);

        let is_liability = matches!(
            acc.account_type,
            crate::api::lunch_money::schema::AccountType::Credit
                | crate::api::lunch_money::schema::AccountType::Loan
                | crate::api::lunch_money::schema::AccountType::OtherLiability
        );

        let target_balance = if is_liability {
            -splitwise_balance
        } else {
            splitwise_balance
        };

        let acc_name = acc.display_name.as_deref().unwrap_or(&acc.name);

        if acc.balance != target_balance {
            has_updates = true;
            if args.dry_run {
                anstream::println! { "  {} ({})  {}~ Would update balance: {} -> {}{}", acc_name, currency, STYLE_WARNING, acc.balance, target_balance, STYLE_WARNING.render_reset() };
            } else {
                anstream::println! { "  {} ({})  ~ Updating balance: {} -> {}...", acc_name, currency, acc.balance, target_balance };
                lm_client
                    .exec(
                        Method::PUT,
                        &format!("manual_accounts/{}", account_id),
                        &crate::api::lunch_money::schema::UpdateManualAccountObject {
                            balance: target_balance,
                        },
                    )
                    .await;
            }
        } else {
            anstream::println! { "  {} ({})  {}✓ Up to date: {}{}", acc_name, currency, STYLE_SUCCESS, acc.balance, STYLE_SUCCESS.render_reset() };
        }
    }

    // List unmapped non-zero balances
    let mut unmapped = Vec::new();
    for (currency, &balance) in &global_balances {
        if !target_accounts.contains_key(currency) && !balance.is_zero() {
            unmapped.push((currency, balance));
        }
    }

    if !unmapped.is_empty() {
        anstream::println! {};
        anstream::println! { "{STYLE_WARNING}⚠️  Unmapped Splitwise balances:{STYLE_WARNING:#}" };
        for (curr, bal) in unmapped {
            anstream::println! { "  • {} {}", bal, curr };
        }
        anstream::println! { "  {STYLE_DIM}To sync these, configure target accounts in splitwise-lunchmoney.toml.{STYLE_DIM:#}" };
    }

    anstream::println! { "{STYLE_DIM}─────────────────────────────────────────────────────────────────{STYLE_DIM:#}" };
    if args.dry_run {
        if has_updates {
            anstream::println! { "{STYLE_WARNING}⚠️  Dry run complete! Changes would be applied to Lunch Money.{STYLE_WARNING:#}" };
        } else {
            anstream::println! { "{STYLE_SUCCESS}✨ Dry run complete! All accounts are already up to date.{STYLE_SUCCESS:#}" };
        }
    } else {
        if has_updates {
            anstream::println! { "{STYLE_SUCCESS}✨ Balance synchronization complete!{STYLE_SUCCESS:#}" };
        } else {
            anstream::println! { "{STYLE_SUCCESS}✨ No balance updates needed. Lunch Money accounts are up to date!{STYLE_SUCCESS:#}" };
        }
    }

    anstream::println! {};
}
