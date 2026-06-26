use crate::style::*;
use anstream::println;
use anyhow::Context;
use rust_decimal::Decimal;
use std::collections::HashMap;

pub(crate) async fn run_sync_balances(
    ctx: &crate::AppContext,
    args: crate::cli::SyncBalancesArgs,
) -> anyhow::Result<()> {
    let config = &ctx.config;

    let sw_client = &ctx.splitwise;
    let lm_client = &ctx.lunch_money;

    println! {};
    println! { "{STYLE_HEADER}🔄 Syncing Splitwise Balances to Lunch Money{STYLE_HEADER:#}" };
    if ctx.dry_run {
        println! { "{STYLE_WARNING}⚠️  Running in DRY RUN mode. No changes will be made to Lunch Money.{STYLE_WARNING:#}" };
    }
    println! { "{STYLE_DIM}─────────────────────────────────────────────────────────────────{STYLE_DIM:#}" };

    println! { "  {STYLE_DIM}Fetching Splitwise friends...{STYLE_DIM:#}" };
    let friends = sw_client.fetch_friends().await?;

    let mut global_balances: HashMap<crate::api::Currency, Decimal> = HashMap::new();
    for friend in friends {
        for bal in friend.balance {
            let currency = bal.currency_code.clone();
            *global_balances.entry(currency).or_insert(Decimal::ZERO) += bal.amount;
        }
    }

    if !config.splitwise.ignored_groups.is_empty() {
        println! { "  {STYLE_DIM}Fetching Splitwise groups...{STYLE_DIM:#}" };
        let groups = sw_client.fetch_groups().await?;

        for group in groups {
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
    let manual_accounts = lm_client.fetch_manual_accounts().await?;

    let target_accounts = crate::commands::resolve_target_accounts(
        &manual_accounts,
        &config.lunch_money.custom_accounts,
    );

    let mut has_updates = false;
    let mut csv_rows = Vec::new();

    for (currency, &account_id) in &target_accounts {
        let acc = match manual_accounts.iter().find(|a| a.id == account_id) {
            Some(a) => a,
            None => {
                anyhow::bail!(
                    "Manual account ID {} for currency '{}' has been deleted or does not exist in Lunch Money.",
                    account_id,
                    currency
                );
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

        let operation = if acc.balance != target_balance {
            "update"
        } else {
            "none"
        };

        csv_rows.push((
            operation,
            account_id,
            acc_name.to_string(),
            currency.to_string(),
            acc.balance,
            target_balance,
        ));

        if acc.balance != target_balance {
            has_updates = true;
            if ctx.dry_run {
                println! { "  {} ({})  {}~ Would update balance: {} -> {}{}", acc_name, currency, STYLE_WARNING, acc.balance, target_balance, STYLE_WARNING.render_reset() };
            } else {
                println! { "  {} ({})  ~ Updating balance: {} -> {}...", acc_name, currency, acc.balance, target_balance };
                lm_client
                    .update_manual_account(account_id, target_balance)
                    .await?;
            }
        } else {
            println! { "  {} ({})  {}✓ Up to date: {}{}", acc_name, currency, STYLE_SUCCESS, acc.balance, STYLE_SUCCESS.render_reset() };
        }
    }

    // Write CSV if requested
    let csv_path = match args.csv {
        Some(Some(path)) => Some(path),
        Some(None) => Some(std::path::PathBuf::from("balances.csv")),
        None => None,
    };

    if let Some(ref csv_path) = csv_path {
        #[derive(serde::Serialize)]
        struct CsvRow<'a> {
            operation: &'static str,
            account_id: crate::api::lunch_money::schema::ManualAccountId,
            account_name: &'a str,
            currency: &'a str,
            old_balance: Decimal,
            new_balance: Decimal,
        }

        let mut wtr = csv::Writer::from_path(csv_path)
            .with_context(|| format!("Failed to create CSV file at '{}'", csv_path.display()))?;

        for (op, account_id, name, curr, old_bal, new_bal) in csv_rows {
            wtr.serialize(CsvRow {
                operation: op,
                account_id,
                account_name: name.as_str(),
                currency: curr.as_str(),
                old_balance: old_bal,
                new_balance: new_bal,
            })
            .context("Failed to write CSV row")?;
        }

        wtr.flush().context("Failed to flush CSV file")?;
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
    if ctx.dry_run {
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
    Ok(())
}
