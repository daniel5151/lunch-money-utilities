use crate::api::lunch_money::schema::ManualAccountsResponse;
use crate::style::*;
use anstream::eprintln;
use anstream::println;
use reqwest::Method;
use rust_decimal::Decimal;
use std::collections::HashMap;

pub(crate) async fn run_sync_balances(args: crate::cli::SyncBalancesArgs) {
    let config = crate::load_config();

    let http_pool = reqwest::Client::new();
    let sw_client =
        crate::api::splitwise::Client::new(http_pool.clone(), config.splitwise.api_key.clone());
    let lm_client =
        crate::api::lunch_money::Client::new(http_pool.clone(), config.lunch_money.api_key.clone());

    println! {};
    println! { "{STYLE_HEADER}🔄 Syncing Splitwise Balances to Lunch Money{STYLE_HEADER:#}" };
    if args.dry_run {
        println! { "{STYLE_WARNING}⚠️  Running in DRY RUN mode. No changes will be made to Lunch Money.{STYLE_WARNING:#}" };
    }
    println! { "{STYLE_DIM}─────────────────────────────────────────────────────────────────{STYLE_DIM:#}" };

    println! { "  {STYLE_DIM}Fetching Splitwise friends...{STYLE_DIM:#}" };
    let friends_res: crate::api::splitwise::schema::FriendsResponse =
        sw_client.fetch("get_friends", &[] as &[(&str, &str)]).await;

    let mut global_balances: HashMap<crate::api::Currency, Decimal> = HashMap::new();
    for friend in friends_res.friends {
        for bal in friend.balance {
            let currency = bal.currency_code.clone();
            *global_balances.entry(currency).or_insert(Decimal::ZERO) += bal.amount;
        }
    }

    if !config.splitwise.ignored_groups.is_empty() {
        println! { "  {STYLE_DIM}Fetching Splitwise groups...{STYLE_DIM:#}" };
        let groups_res: crate::api::splitwise::schema::GroupResponse =
            sw_client.fetch("get_groups", &[] as &[(&str, &str)]).await;

        for group in groups_res.groups {
            if config
                .splitwise
                .is_group_ignored(group.id, Some(&group.name))
            {
                if let Some(members) = &group.members {
                    if let Some(member) = members.iter().find(|m| m.id == config.splitwise.user_id)
                    {
                        for bal in &member.balance {
                            let currency = bal.currency_code.clone();
                            *global_balances.entry(currency).or_insert(Decimal::ZERO) -= bal.amount;
                        }
                    }
                }
            }
        }
    }

    println! { "  {STYLE_DIM}Fetching Lunch Money manual accounts...{STYLE_DIM:#}" };
    let accounts_res: ManualAccountsResponse = lm_client
        .fetch("manual_accounts", &[] as &[(&str, &str)])
        .await;

    let target_accounts = crate::commands::resolve_target_accounts(
        &accounts_res,
        &config.lunch_money.custom_accounts,
    );

    let mut has_updates = false;

    for (currency, &account_id) in &target_accounts {
        let acc = match accounts_res
            .manual_accounts
            .iter()
            .find(|a| a.id == account_id)
        {
            Some(a) => a,
            None => {
                eprintln! {};
                eprintln! { "{STYLE_ERROR}❌ Error:{STYLE_ERROR:#} Manual account ID {} for currency '{}' has been deleted or does not exist in Lunch Money.", account_id, currency };
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
                println! { "  {} ({})  {}~ Would update balance: {} -> {}{}", acc_name, currency, STYLE_WARNING, acc.balance, target_balance, STYLE_WARNING.render_reset() };
            } else {
                println! { "  {} ({})  ~ Updating balance: {} -> {}...", acc_name, currency, acc.balance, target_balance };
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
            println! { "  {} ({})  {}✓ Up to date: {}{}", acc_name, currency, STYLE_SUCCESS, acc.balance, STYLE_SUCCESS.render_reset() };
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
        println! {};
        println! { "{STYLE_WARNING}⚠️  Unmapped Splitwise balances:{STYLE_WARNING:#}" };
        for (curr, bal) in unmapped {
            println! { "  • {} {}", bal, curr };
        }
        println! { "  {STYLE_DIM}To sync these, create 'Splitwise <CURRENCY>' manual accounts in Lunch Money or configure custom accounts.{STYLE_DIM:#}" };
    }

    println! { "{STYLE_DIM}─────────────────────────────────────────────────────────────────{STYLE_DIM:#}" };
    if args.dry_run {
        if has_updates {
            println! { "{STYLE_WARNING}⚠️  Dry run complete! Changes would be applied to Lunch Money.{STYLE_WARNING:#}" };
        } else {
            println! { "{STYLE_SUCCESS}✨ Dry run complete! All accounts are already up to date.{STYLE_SUCCESS:#}" };
        }
    } else {
        if has_updates {
            println! { "{STYLE_SUCCESS}✨ Balance synchronization complete!{STYLE_SUCCESS:#}" };
        } else {
            println! { "{STYLE_SUCCESS}✨ No balance updates needed. Lunch Money accounts are up to date!{STYLE_SUCCESS:#}" };
        }
    }

    println! {};
}
