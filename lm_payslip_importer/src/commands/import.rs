use crate::payslip::convert_pdf_to_pages;
use crate::payslip::parse_page_tables;
use crate::style::*;
use anstream::eprintln;
use anstream::println;
use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use jiff::civil::Date;
use lunch_money::client::Client as LunchMoneyClient;
use lunch_money::client::TooManyRequestsPolicy;
use lunch_money::core::CategoryId;
use lunch_money::core::ManualAccountId;
use lunch_money::core::PlaidAccountId;
use lunch_money::core::TransactionId;
use lunch_money::transactions::query_params::TransactionQuery;
use lunch_money::transactions::schemas::InsertObject;
use lunch_money::transactions::schemas::SplitTransactionObject;
use lunch_money::transactions::schemas::SplitTransactionPayload;
use lunch_money::transactions::schemas::TransactionStatus;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::collections::HashSet;
use std::env;

#[derive(Debug, Clone, Copy)]
pub enum ResolvedAccount {
    Plaid(PlaidAccountId),
    Manual(ManualAccountId),
}

pub struct SplitComponent {
    pub description: String,
    pub amount: Decimal,
    pub category_id: CategoryId,
    pub category_name: String,
}

pub(crate) async fn run_import(
    config: crate::config::Config,
    cli: crate::cli::ImportArgs,
) -> Result<()> {
    let api_key_opt = config
        .lunch_money
        .api_key
        .clone()
        .or_else(|| env::var("LUNCH_MONEY_API_KEY").ok())
        .filter(|s| !s.trim().is_empty());

    if api_key_opt.is_none() && !cli.dry_run {
        anyhow::bail!(
            "Lunch Money API key not set in lm_payslip_importer.toml or LUNCH_MONEY_API_KEY environment variable. Please set it or run with --dry-run."
        );
    }

    println! { "{STYLE_HEADER}📄 Reading payslip PDF: {}{STYLE_HEADER:#}", cli.payslip_pdf.display() };
    let pages = convert_pdf_to_pages(&cli.payslip_pdf)?;

    for &p in &cli.pages {
        if p == 0 || p > pages.len() {
            anyhow::bail!(
                "Requested page number {} is invalid. The PDF has {} pages.",
                p,
                pages.len()
            );
        }
    }

    let mut parsed_pages = Vec::new();

    for (i, page_text) in pages.iter().enumerate() {
        let page_num = i + 1;
        if !cli.pages.is_empty() && !cli.pages.contains(&page_num) {
            continue;
        }
        let page_text = page_text.trim();
        if page_text.is_empty() {
            continue;
        }
        let parsed = parse_page_tables(page_text, page_num)?;
        parsed_pages.push(parsed);
    }

    println! { "Parsed {} pages.", parsed_pages.len() };
    if parsed_pages.is_empty() {
        return Ok(());
    }

    let client = if let Some(api_key) = api_key_opt {
        println! { "Initializing Lunch Money client..." };
        let http = reqwest::Client::new();
        Some(LunchMoneyClient::new(
            http,
            api_key,
            TooManyRequestsPolicy::Retry {
                max_retries: 5,
                initial_delay: std::time::Duration::from_secs(2),
            },
        ))
    } else {
        None
    };

    let resolved_cats = if let Some(ref client_ref) = client {
        println! { "Resolving Lunch Money category names..." };
        let query = lunch_money::categories::query_params::CategoryQuery::builder()
            .format("flattened".to_string())
            .build();
        let lm_categories = client_ref
            .fetch_categories(&query)
            .await
            .context("Failed to fetch Lunch Money categories")?;

        let mut map = HashMap::new();
        for (payslip_item, lm_cat_name) in &config.mapping {
            let resolved = find_category(&lm_categories, lm_cat_name).with_context(|| {
                format!(
                    "Failed to resolve mapping for payslip item '{}'",
                    payslip_item
                )
            })?;
            map.insert(payslip_item.clone(), resolved);
        }
        map
    } else {
        let mut map = HashMap::new();
        for (payslip_item, lm_cat_name) in &config.mapping {
            map.insert(payslip_item.clone(), (lm_cat_name.clone(), CategoryId(0)));
        }
        map
    };

    let resolved_net_zero_acct = if let Some(ref client_ref) = client {
        let acct_name = &config.lunch_money.net_zero_account;
        println! { "Resolving Lunch Money account name '{}'...", acct_name };
        Some(
            resolve_account(client_ref, acct_name)
                .await?
                .ok_or_else(|| {
                    anyhow!(
                        "Configured net-zero account '{}' does not exist in Lunch Money.",
                        acct_name
                    )
                })?,
        )
    } else {
        None
    };

    let resolved_rsu_acct = if let Some(ref client_ref) = client {
        let acct_name = &config.lunch_money.rsu_account;
        println! { "Resolving Lunch Money RSU account name '{}'...", acct_name };
        Some(
            resolve_account(client_ref, acct_name)
                .await?
                .ok_or_else(|| {
                    anyhow!(
                        "Configured RSU account '{}' does not exist in Lunch Money.",
                        acct_name
                    )
                })?,
        )
    } else {
        None
    };

    let mut matched_tx_ids = HashSet::new();

    let mut min_date = parsed_pages[0].check_date;
    let mut max_date = parsed_pages[0].check_date;
    for page in &parsed_pages {
        if page.check_date < min_date {
            min_date = page.check_date;
        }
        if page.check_date > max_date {
            max_date = page.check_date;
        }
    }

    let start_date = min_date
        .checked_sub(jiff::Span::new().days(3))
        .map_err(|e| anyhow!("Failed to subtract days: {}", e))?;
    let end_date = max_date
        .checked_add(jiff::Span::new().days(3))
        .map_err(|e| anyhow!("Failed to add days: {}", e))?;

    let checking_txs = if let Some(ref client_ref) = client {
        if let Some(ref checking_acct) = resolved_net_zero_acct {
            let query = match checking_acct {
                ResolvedAccount::Plaid(id) => TransactionQuery::builder()
                    .start_date(start_date.to_string())
                    .end_date(end_date.to_string())
                    .limit(1000)
                    .plaid_account_id(*id)
                    .build(),
                ResolvedAccount::Manual(id) => TransactionQuery::builder()
                    .start_date(start_date.to_string())
                    .end_date(end_date.to_string())
                    .limit(1000)
                    .manual_account_id(*id)
                    .build(),
            };

            println! { "Fetching transactions for net-zero account '{}' between {} and {}...", config.lunch_money.net_zero_account, start_date, end_date };
            let tx_response = client_ref
                .fetch_transactions::<serde_json::Value, String>(&query)
                .await
                .context("Failed to fetch checking transactions")?;
            println! { "Fetched {} checking transactions.", tx_response.transactions.len() };
            tx_response.transactions
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    let rsu_txs = if let Some(ref client_ref) = client {
        if let Some(ref rsu_acct) = resolved_rsu_acct {
            let query = match rsu_acct {
                ResolvedAccount::Plaid(id) => TransactionQuery::builder()
                    .start_date(start_date.to_string())
                    .end_date(end_date.to_string())
                    .limit(1000)
                    .plaid_account_id(*id)
                    .build(),
                ResolvedAccount::Manual(id) => TransactionQuery::builder()
                    .start_date(start_date.to_string())
                    .end_date(end_date.to_string())
                    .limit(1000)
                    .manual_account_id(*id)
                    .build(),
            };

            println! { "Fetching transactions for RSU account '{}' between {} and {}...", config.lunch_money.rsu_account, start_date, end_date };
            let tx_response = client_ref
                .fetch_transactions::<serde_json::Value, String>(&query)
                .await
                .context("Failed to fetch RSU transactions")?;
            println! { "Fetched {} RSU transactions.", tx_response.transactions.len() };
            tx_response.transactions
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    for payslip in &parsed_pages {
        let check_date_str = payslip.check_date.to_string();
        let net_pay = payslip.net_pay;

        println! {
            "\n{STYLE_HEADER}Matching payslip Page {}: Check Date = {}, Net Pay = {}{STYLE_HEADER:#}",
            payslip.page_num, check_date_str, net_pay
        };

        // Detect if this is an RSU vest page
        let rsu_earn = payslip
            .earnings
            .iter()
            .find(|earn| earn.description.to_lowercase().contains("restricted stock"));

        let is_rsu_vest = if let Some(earn) = rsu_earn {
            let amount = earn.values.get("Amount").copied().unwrap_or(Decimal::ZERO);
            !amount.is_zero()
        } else {
            false
        };

        if is_rsu_vest {
            let rsu_earn = rsu_earn.unwrap();
            let rsu_amount = rsu_earn
                .values
                .get("Amount")
                .copied()
                .unwrap_or(Decimal::ZERO);

            // Calculate total taxes and gather tax split components
            let mut tax_components = Vec::new();
            for tax in &payslip.employee_taxes {
                let amount = tax.values.get("Amount").copied().unwrap_or(Decimal::ZERO);
                if !amount.is_zero() {
                    let (cat_name, cat_id) = map_category(&tax.description, &resolved_cats)?;
                    tax_components.push(SplitComponent {
                        description: format!(
                            "{} - {}",
                            config.lunch_money.payslip_payee, tax.description
                        ),
                        amount,
                        category_id: cat_id,
                        category_name: cat_name,
                    });
                }
            }

            let total_taxes: Decimal = tax_components.iter().map(|c| c.amount).sum();
            let net_value = rsu_amount - total_taxes;
            let parent_amount = -net_value; // In Lunch Money, credit is negative

            let (rsu_cat_name, rsu_cat_id) = map_category(&rsu_earn.description, &resolved_cats)?;
            let mut rsu_components = vec![SplitComponent {
                description: format!(
                    "{} - {}",
                    config.lunch_money.payslip_payee, rsu_earn.description
                ),
                amount: -rsu_amount,
                category_id: rsu_cat_id,
                category_name: rsu_cat_name,
            }];
            rsu_components.extend(tax_components);

            // Match RSU vest to auto-imported Plaid transaction(s)
            let matched_plaid_rsu_txs = if let Some(ref rsu_acct) = resolved_rsu_acct {
                let start_window = payslip
                    .check_date
                    .checked_sub(jiff::Span::new().days(3))
                    .map_err(|e| anyhow!("Failed to subtract days: {}", e))?;
                let end_window = payslip
                    .check_date
                    .checked_add(jiff::Span::new().days(3))
                    .map_err(|e| anyhow!("Failed to add days: {}", e))?;

                rsu_txs
                    .iter()
                    .filter(|tx| {
                        let matches_acct = match rsu_acct {
                            ResolvedAccount::Plaid(id) => tx.plaid_account_id == Some(*id),
                            ResolvedAccount::Manual(id) => tx.manual_account_id == Some(*id),
                        };
                        matches_acct
                            && tx.date >= start_window
                            && tx.date <= end_window
                            && tx.amount.is_zero()
                            && tx.payee.eq_ignore_ascii_case(&config.lunch_money.rsu_payee_match)
                    })
                    .cloned()
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            };

            let mut synthetic_tx_date = payslip.check_date;
            if !matched_plaid_rsu_txs.is_empty() {
                synthetic_tx_date = matched_plaid_rsu_txs[0].date;
                let matched_ids: Vec<String> = matched_plaid_rsu_txs
                    .iter()
                    .map(|tx| tx.id.to_string())
                    .collect();
                println! {
                    "  {STYLE_SUCCESS}✅ Matched RSU vest to auto-imported Plaid transaction(s): {}{STYLE_SUCCESS:#}",
                    matched_ids.join(", ")
                };
            }

            if cli.dry_run {
                println! {
                    "  {STYLE_INFO}ℹ️ [Dry Run] RSU Vest Detected. Creating synthetic transaction in account '{}'.{STYLE_INFO:#}",
                    &config.lunch_money.rsu_account
                };
                if !matched_plaid_rsu_txs.is_empty() {
                    let matched_ids: Vec<String> = matched_plaid_rsu_txs
                        .iter()
                        .map(|tx| format!("#{}", tx.id))
                        .collect();
                    println! {
                        "  {STYLE_INFO}ℹ️ [Dry Run] Aligned date to matched Plaid transaction date ({}) and will reference companion Plaid transaction(s) in notes: {}{STYLE_INFO:#}",
                        synthetic_tx_date, matched_ids.join(", ")
                    };
                }
                println! {
                    "\n  {STYLE_HEADER}Plan: Create transaction (Date: {}, Amount: {}, Payee: {}){STYLE_HEADER:#}",
                    synthetic_tx_date, parent_amount, config.lunch_money.payslip_payee
                };
                for comp in &rsu_components {
                    let sign_style = if comp.amount.is_sign_negative() {
                        STYLE_SUCCESS
                    } else {
                        STYLE_WARNING
                    };
                    println! {
                        "    {sign_style}{:>10}  {:<20}  {}{sign_style:#}",
                        comp.amount, comp.category_name, comp.description
                    };
                }
            } else {
                if let Some(ref client_ref) = client {
                    if let Some(ref rsu_acct) = resolved_rsu_acct {
                        println! {
                            "  RSU Vest Detected. Creating synthetic transaction in RSU account '{}'...{STYLE_INFO:#}",
                            &config.lunch_money.rsu_account
                        };

                        let notes = if !matched_plaid_rsu_txs.is_empty() {
                            let companion_ids: Vec<String> = matched_plaid_rsu_txs
                                .iter()
                                .map(|tx| format!("#{}", tx.id))
                                .collect();
                            format!(
                                "Synthetic compensation transaction for RSU vest split. Companion to auto-imported vest transaction(s): {}.",
                                companion_ids.join(", ")
                            )
                        } else {
                            "Synthetic transaction for RSU vest split".to_string()
                        };

                        let insert_tx = insert_transaction(
                            synthetic_tx_date,
                            parent_amount,
                            config.lunch_money.payslip_payee.clone(),
                            notes,
                            rsu_cat_id,
                            rsu_acct,
                        )?;

                        let insert_resp = client_ref
                            .insert_transactions::<serde_json::Value, String, serde_json::Value, String>(&[insert_tx])
                            .await
                            .context("Failed to insert synthetic RSU transaction")?;

                        if insert_resp.transactions.is_empty() {
                            println! {
                                "  {STYLE_ERROR}❌ Failed to create synthetic RSU transaction: no transactions returned.{STYLE_ERROR:#}"
                            };
                            continue;
                        }

                        let new_tx = &insert_resp.transactions[0];
                        println! {
                            "  {STYLE_SUCCESS}✅ Created synthetic RSU transaction: ID = {}, Date = {}, Payee = \"{}\"{STYLE_SUCCESS:#}",
                            new_tx.id, new_tx.date, new_tx.payee
                        };

                        let child_txs: Vec<SplitTransactionObject> = rsu_components
                            .iter()
                            .map(|comp| {
                                SplitTransactionObject::builder()
                                    .amount(comp.amount)
                                    .maybe_payee(Some(comp.description.clone()))
                                    .maybe_category_id(Some(comp.category_id))
                                    .maybe_notes(Some(String::new()))
                                    .build()
                            })
                            .collect();

                        let payload = SplitTransactionPayload {
                            child_transactions: child_txs,
                        };

                        println! {
                            "  Splitting transaction ID {} in Lunch Money...",
                            new_tx.id
                        };
                        match client_ref
                            .split_transaction::<serde_json::Value, String>(new_tx.id, &payload)
                            .await
                        {
                            Ok(_) => {
                                println! {
                                    "  {STYLE_SUCCESS}✅ Successfully split RSU transaction ID {}!{STYLE_SUCCESS:#}",
                                    new_tx.id
                                };
                            }
                            Err(e) => {
                                eprintln! {
                                    "  {STYLE_ERROR}❌ Error splitting RSU transaction ID {}: {}{STYLE_ERROR:#}",
                                    new_tx.id, e
                                };
                            }
                        }


                    } else {
                        println! {
                            "  {STYLE_ERROR}❌ RSU account not resolved in configuration.{STYLE_ERROR:#}"
                        };
                    }
                } else {
                    println! {
                        "  {STYLE_WARNING}⚠️ Lunch Money API key is not set. Skipping RSU transaction.{STYLE_WARNING:#}"
                    };
                }
            }
            continue;
        }

        let mut tx_id = TransactionId::from(0);
        let mut tx_date = payslip.check_date;
        let mut tx_amount = -payslip.net_pay;
        let mut tx_payee = config.lunch_money.payslip_payee.clone();

        if let Some(ref client_ref) = client {
            let start_window = payslip
                .check_date
                .checked_sub(jiff::Span::new().days(3))
                .map_err(|e| anyhow!("Failed to subtract days: {}", e))?;
            let end_window = payslip
                .check_date
                .checked_add(jiff::Span::new().days(3))
                .map_err(|e| anyhow!("Failed to add days: {}", e))?;

            let mut best_match = None;
            for tx in &checking_txs {
                if matched_tx_ids.contains(&tx.id) {
                    continue;
                }

                if tx.date < start_window || tx.date > end_window {
                    continue;
                }

                // In Lunch Money, credit amounts are negative
                let tx_amount_abs = tx.amount.abs();

                if (tx_amount_abs - net_pay).abs() < Decimal::new(1, 2) {
                    best_match = Some(tx);
                    break;
                }
            }

            if let Some(tx) = best_match {
                matched_tx_ids.insert(tx.id);
                println! {
                    "  {STYLE_SUCCESS}✅ Matched to Lunch Money transaction: ID = {}, Date = {}, Amount = {}, Payee = \"{}\"{STYLE_SUCCESS:#}",
                    tx.id, tx.date, tx.amount, tx.payee
                };

                tx_id = tx.id;
                tx_date = tx.date;
                tx_amount = tx.amount;
                tx_payee = tx.payee.clone();
            } else {
                // If no matching transaction is found and this is a net-zero check, create a synthetic transaction
                if payslip.net_pay.is_zero() {
                    let account_name = &config.lunch_money.net_zero_account;

                    if cli.dry_run {
                        println! {
                            "  {STYLE_INFO}ℹ️ [Dry Run] No match found. Would create synthetic $0.00 transaction for Page {} in account '{}'.{STYLE_INFO:#}",
                            payslip.page_num, account_name
                        };
                        tx_id = TransactionId(0);
                        tx_date = payslip.check_date;
                        tx_amount = Decimal::ZERO;
                        tx_payee = config.lunch_money.payslip_payee.clone();
                    } else {
                        let acct = match resolved_net_zero_acct {
                            Some(acct) => acct,
                            None => {
                                println! {
                                    "  {STYLE_ERROR}❌ Failed to resolve net-zero account name '{}'{STYLE_ERROR:#}",
                                    account_name
                                };
                                continue;
                            }
                        };

                        println! {
                            "  {STYLE_INFO}ℹ️ No match found. Creating synthetic $0.00 transaction for Page {} in account '{}'...{STYLE_INFO:#}",
                            payslip.page_num, account_name
                        };

                        let insert_tx = insert_transaction_for_zero_pay(
                            payslip.check_date,
                            &acct,
                            config.lunch_money.payslip_payee.clone(),
                        )?;
                        let insert_resp = client_ref
                            .insert_transactions::<serde_json::Value, String, serde_json::Value, String>(&[insert_tx])
                            .await
                            .context("Failed to insert synthetic zero-dollar transaction")?;

                        if insert_resp.transactions.is_empty() {
                            println! {
                                "  {STYLE_ERROR}❌ Failed to create synthetic zero-dollar transaction: no transactions returned in response.{STYLE_ERROR:#}"
                            };
                            continue;
                        }

                        let new_tx = &insert_resp.transactions[0];
                        println! {
                            "  {STYLE_SUCCESS}✅ Created synthetic zero-dollar transaction: ID = {}, Date = {}, Payee = \"{}\"{STYLE_SUCCESS:#}",
                            new_tx.id, new_tx.date, new_tx.payee
                        };

                        tx_id = new_tx.id;
                        tx_date = new_tx.date;
                        tx_amount = new_tx.amount;
                        tx_payee = new_tx.payee.clone();
                    }
                } else {
                    println! {
                        "  {STYLE_ERROR}❌ No matching Lunch Money transaction found for Page {}.{STYLE_ERROR:#}",
                        payslip.page_num
                    };
                    continue;
                }
            }
        } else {
            println! {
                "  {STYLE_WARNING}⚠️ Lunch Money API key is not set. Skipping transaction matching. Using dummy values for dry-run simulation.{STYLE_WARNING:#}"
            };
        }

        let mut components = Vec::new();

        // Helper to check for imputed income components
        let is_imputed_income = |desc: &str| -> bool {
            let desc_trimmed = desc.trim();
            if desc_trimmed.starts_with('*') {
                return true;
            }
            let desc_lower = desc_trimmed.to_lowercase();
            config
                .imputed_income
                .exceptions
                .iter()
                .any(|exception| desc_lower.contains(&exception.to_lowercase()))
        };

        // 1. Earnings (credits - negative amount in Lunch Money)
        for earn in &payslip.earnings {
            let amount = earn.values.get("Amount").copied().unwrap_or(Decimal::ZERO);
            if !amount.is_zero() {
                let (cat_name, cat_id) = map_category(&earn.description, &resolved_cats)?;
                components.push(SplitComponent {
                    description: format!("{} - {}", config.lunch_money.payslip_payee, earn.description),
                    amount: -amount,
                    category_id: cat_id,
                    category_name: cat_name.clone(),
                });

                if is_imputed_income(&earn.description) {
                    components.push(SplitComponent {
                        description: format!("{} - {} Offset", config.lunch_money.payslip_payee, earn.description),
                        amount,
                        category_id: cat_id,
                        category_name: cat_name,
                    });
                }
            }
        }

        // 2. Pre Tax Deductions (debits - positive amount in Lunch Money)
        for ded in &payslip.pre_tax_deductions {
            let amount = ded.values.get("Amount").copied().unwrap_or(Decimal::ZERO);
            if !amount.is_zero() {
                let (cat_name, cat_id) = map_category(&ded.description, &resolved_cats)?;
                components.push(SplitComponent {
                    description: format!("{} - {}", config.lunch_money.payslip_payee, ded.description),
                    amount,
                    category_id: cat_id,
                    category_name: cat_name.clone(),
                });

                if is_imputed_income(&ded.description) {
                    components.push(SplitComponent {
                        description: format!("{} - {} Offset", config.lunch_money.payslip_payee, ded.description),
                        amount: -amount,
                        category_id: cat_id,
                        category_name: cat_name,
                    });
                }
            }
        }

        // 3. Employee Taxes (debits - positive amount in Lunch Money)
        for tax in &payslip.employee_taxes {
            let amount = tax.values.get("Amount").copied().unwrap_or(Decimal::ZERO);
            if !amount.is_zero() {
                let (cat_name, cat_id) = map_category(&tax.description, &resolved_cats)?;
                components.push(SplitComponent {
                    description: format!("{} - {}", config.lunch_money.payslip_payee, tax.description),
                    amount,
                    category_id: cat_id,
                    category_name: cat_name.clone(),
                });

                if is_imputed_income(&tax.description) {
                    components.push(SplitComponent {
                        description: format!("{} - {} Offset", config.lunch_money.payslip_payee, tax.description),
                        amount: -amount,
                        category_id: cat_id,
                        category_name: cat_name,
                    });
                }
            }
        }

        // 4. Post Tax Deductions (debits or credits)
        for ded in &payslip.post_tax_deductions {
            let amount = ded.values.get("Amount").copied().unwrap_or(Decimal::ZERO);
            if !amount.is_zero() {
                let (cat_name, cat_id) = map_category(&ded.description, &resolved_cats)?;
                components.push(SplitComponent {
                    description: format!("{} - {}", config.lunch_money.payslip_payee, ded.description),
                    amount,
                    category_id: cat_id,
                    category_name: cat_name.clone(),
                });

                if is_imputed_income(&ded.description) {
                    components.push(SplitComponent {
                        description: format!("{} - {} Offset", config.lunch_money.payslip_payee, ded.description),
                        amount: -amount,
                        category_id: cat_id,
                        category_name: cat_name,
                    });
                }
            }
        }

        // Validate sum
        let comp_sum: Decimal = components.iter().map(|c| c.amount).sum();
        let diff = comp_sum - tx_amount;

        if !diff.is_zero() {
            anyhow::bail!(
                "Sum of components ({}) does not match transaction amount ({}) exactly. Diff: {}",
                comp_sum,
                tx_amount,
                diff
            );
        }

        let child_txs: Vec<SplitTransactionObject> = components
            .iter()
            .map(|comp| {
                SplitTransactionObject::builder()
                    .amount(comp.amount)
                    .maybe_payee(Some(comp.description.clone()))
                    .maybe_category_id(Some(comp.category_id))
                    .maybe_notes(Some(String::new()))
                    .build()
            })
            .collect();

        let payload = SplitTransactionPayload {
            child_transactions: child_txs,
        };

        if cli.dry_run {
            println! {
                "\n  {STYLE_HEADER}Plan: Split transaction ID {} (Date: {}, Amount: {}, Payee: {}){STYLE_HEADER:#}",
                tx_id, tx_date, tx_amount, tx_payee
            };
            for comp in &components {
                let sign_style = if comp.amount.is_sign_negative() {
                    STYLE_SUCCESS
                } else {
                    STYLE_WARNING
                };
                println! {
                    "    {sign_style}{:>10}  {:<20}  {}{sign_style:#}",
                    comp.amount, comp.category_name, comp.description
                };
            }
        } else {
            println! {
                "  Splitting transaction ID {} in Lunch Money...",
                tx_id
            };
            if let Some(ref client_ref) = client {
                match client_ref
                    .split_transaction::<serde_json::Value, String>(tx_id, &payload)
                    .await
                {
                    Ok(_) => {
                        println! { "  {STYLE_SUCCESS}✅ Successfully split transaction ID {}!{STYLE_SUCCESS:#}", tx_id };
                    }
                    Err(e) => {
                        eprintln! { "  {STYLE_ERROR}❌ Error splitting transaction ID {}: {}{STYLE_ERROR:#}", tx_id, e };
                    }
                }
            } else {
                println! {
                    "  {STYLE_ERROR}❌ Cannot split transaction: Lunch Money API key not set.{STYLE_ERROR:#}"
                };
            }
        }
    }

    Ok(())
}

fn map_category(
    desc: &str,
    resolved_mapping: &HashMap<String, (String, CategoryId)>,
) -> Result<(String, CategoryId)> {
    if let Some(val) = resolved_mapping.get(desc) {
        return Ok(val.clone());
    }

    // Try case-insensitive lookup as fallback
    let desc_lower = desc.to_lowercase();
    for (k, v) in resolved_mapping {
        if k.to_lowercase() == desc_lower {
            return Ok(v.clone());
        }
    }

    anyhow::bail!(
        "No Lunch Money category mapping found for payslip item '{}'. Please add it to the [mapping] section of lm_payslip_importer.toml.",
        desc
    )
}

/// Resolve a configured account name to a Plaid or manual account id, matching
/// case-insensitively against either the account name or its display name.
/// Plaid accounts are checked first, then manual accounts. Returns `Ok(None)`
/// when no account matches so callers can attach their own context.
async fn resolve_account(
    client: &LunchMoneyClient,
    acct_name: &str,
) -> Result<Option<ResolvedAccount>> {
    let name_matches = |name: &str, display_name: &Option<String>| {
        name.eq_ignore_ascii_case(acct_name)
            || display_name
                .as_ref()
                .map(|d| d.eq_ignore_ascii_case(acct_name))
                .unwrap_or(false)
    };

    let plaid_accts = client
        .fetch_plaid_accounts()
        .await
        .context("Failed to fetch Plaid accounts")?;
    for acct in plaid_accts {
        if name_matches(&acct.name, &acct.display_name) {
            return Ok(Some(ResolvedAccount::Plaid(acct.id)));
        }
    }

    let manual_accts = client
        .fetch_manual_accounts()
        .await
        .context("Failed to fetch manual accounts")?;
    for acct in manual_accts {
        if name_matches(&acct.name, &acct.display_name) {
            return Ok(Some(ResolvedAccount::Manual(acct.id)));
        }
    }

    Ok(None)
}

fn find_category(
    categories: &[lunch_money::categories::schemas::Category],
    name: &str,
) -> Result<(String, CategoryId)> {
    let matches: Vec<_> = categories
        .iter()
        .filter(|c| c.name == name && !c.archived && !c.is_group)
        .collect();

    if matches.is_empty() {
        anyhow::bail!(
            "Configured Lunch Money category '{}' does not exist or is archived.",
            name
        );
    } else if matches.len() > 1 {
        let mut msg = format!(
            "Multiple active Lunch Money categories found with the name '{}':\n",
            name
        );
        for m in matches {
            msg.push_str(&format!("  • ID: {} (is_group: {})\n", m.id, m.is_group));
        }
        msg.push_str("Please rename one of them to resolve ambiguity.");
        anyhow::bail!("{}", msg);
    } else {
        Ok((matches[0].name.clone(), matches[0].id))
    }
}

fn insert_transaction(
    date: Date,
    amount: Decimal,
    payee: String,
    notes: String,
    category_id: CategoryId,
    resolved_acct: &ResolvedAccount,
) -> Result<InsertObject> {
    let plaid_id = match resolved_acct {
        ResolvedAccount::Plaid(id) => Some(*id),
        _ => None,
    };
    let manual_id = match resolved_acct {
        ResolvedAccount::Manual(id) => Some(*id),
        _ => None,
    };

    Ok(InsertObject::builder()
        .date(date)
        .amount(amount)
        .payee(payee)
        .notes(notes)
        .status(TransactionStatus::Reviewed)
        .maybe_category_id(Some(category_id))
        .maybe_plaid_account_id(plaid_id)
        .maybe_manual_account_id(manual_id)
        .build())
}

fn insert_transaction_for_zero_pay(
    date: Date,
    resolved_acct: &ResolvedAccount,
    payee: String,
) -> Result<InsertObject> {
    let plaid_id = match resolved_acct {
        ResolvedAccount::Plaid(id) => Some(*id),
        _ => None,
    };
    let manual_id = match resolved_acct {
        ResolvedAccount::Manual(id) => Some(*id),
        _ => None,
    };

    Ok(InsertObject::builder()
        .date(date)
        .amount(Decimal::ZERO)
        .payee(payee)
        .notes("Synthetic zero-dollar transaction for relocation/imputed tax split".to_string())
        .status(TransactionStatus::Reviewed)
        .maybe_plaid_account_id(plaid_id)
        .maybe_manual_account_id(manual_id)
        .build())
}
