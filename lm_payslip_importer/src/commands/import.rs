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
use lunch_money::core::TagId;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResolvedAccount {
    Plaid(PlaidAccountId),
    Manual(ManualAccountId),
}

struct PageToProcess {
    pdf_path: std::path::PathBuf,
    page: crate::payslip::ParsedPage,
    kind: crate::payslip::PayslipKind,
}

struct ResolvedBackend {
    backend: crate::config::BackendConfig,
    resolved_cats: HashMap<String, (String, CategoryId)>,
    resolved_net_zero_acct: Option<ResolvedAccount>,
    resolved_rsu_acct: Option<ResolvedAccount>,
}

pub struct SplitComponent {
    pub description: String,
    pub amount: Decimal,
    pub category_id: CategoryId,
    pub category_name: String,
}

/// Outcome of an interactive confirmation prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Decision {
    /// Perform the proposed operation.
    Yes,
    /// Skip this operation and move on to the next one.
    Skip,
    /// Abort the whole import; perform no further operations.
    Stop,
}

/// Ask the user whether to perform the just-printed operation. Presented only
/// in interactive mode, and always *after* the proposed operation has been
/// printed (a-la dry run) so the user can review it before deciding. A
/// cancelled prompt (Esc / Ctrl-C) is treated as Stop, so bailing out of the
/// menu never silently performs a mutation.
fn confirm_operation() -> Decision {
    const YES: &str = "Yes — perform this operation";
    const SKIP: &str = "Skip — leave this one and continue";
    const STOP: &str = "Stop — cancel and perform no further operations";

    match inquire::Select::new("Proceed with the operation above?", vec![YES, SKIP, STOP])
        .with_help_message("Review the proposed operation above, then choose")
        .prompt()
    {
        Ok(YES) => Decision::Yes,
        Ok(SKIP) => Decision::Skip,
        _ => Decision::Stop,
    }
}

pub(crate) async fn run_import(
    config: crate::config::Config,
    cli: crate::cli::ImportArgs,
) -> Result<()> {
    let api_key_opt = config
        .global
        .api_key
        .clone()
        .or_else(|| env::var("LUNCH_MONEY_API_KEY").ok())
        .filter(|s| !s.trim().is_empty());

    if api_key_opt.is_none() && !cli.dry_run {
        anyhow::bail!(
            "Lunch Money API key not set in lm_payslip_importer.toml or LUNCH_MONEY_API_KEY environment variable. Please set it or run with --dry-run."
        );
    }

    // `--page`/`--from-page` select pages within a single document, so they are
    // ambiguous once more than one PDF is supplied (they would otherwise be
    // applied to every file, and a page that is out of range in any one file
    // would abort the whole run). Reject the combination up front with a clear
    // message rather than silently applying the filter to all inputs.
    if cli.payslip_pdfs.len() > 1 && (!cli.pages.is_empty() || cli.from_page.is_some()) {
        anyhow::bail!(
            "--page/--from-page can only be used when importing a single PDF (got {} files). \
             Re-run with one PDF to target specific pages.",
            cli.payslip_pdfs.len()
        );
    }

    let mut pages_to_process = Vec::new();
    for pdf_path in &cli.payslip_pdfs {
        println! { "{STYLE_HEADER}📄 Reading payslip PDF: {}{STYLE_HEADER:#}", pdf_path.display() };
        let kind =
            crate::payslip::detect_kind(pdf_path)?.unwrap_or(crate::payslip::PayslipKind::Workday);
        println! { "  Detected payslip provider: {kind}" };

        let all_pages = crate::payslip::parse_pdf(pdf_path, kind)?;
        let total_pages = all_pages.len();

        for &p in &cli.pages {
            if p == 0 || p > total_pages {
                anyhow::bail!(
                    "Requested page number {} is invalid. PDF '{}' has {} pages.",
                    p,
                    pdf_path.display(),
                    total_pages
                );
            }
        }

        if let Some(from_page) = cli.from_page {
            if from_page == 0 || from_page > total_pages {
                anyhow::bail!(
                    "Requested start page number {} is invalid. PDF '{}' has {} pages.",
                    from_page,
                    pdf_path.display(),
                    total_pages
                );
            }
        }

        for parsed in all_pages {
            let page_num = parsed.page_num;
            if !cli.pages.is_empty() && !cli.pages.contains(&page_num) {
                continue;
            }
            if let Some(from_page) = cli.from_page {
                if page_num < from_page {
                    continue;
                }
            }
            pages_to_process.push(PageToProcess {
                pdf_path: pdf_path.clone(),
                page: parsed,
                kind,
            });
        }
    }

    println! { "Parsed {} pages across {} files.", pages_to_process.len(), cli.payslip_pdfs.len() };
    if pages_to_process.is_empty() {
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

    let mut unique_kinds = HashSet::new();
    for ptp in &pages_to_process {
        unique_kinds.insert(ptp.kind);
    }

    let mut resolved_backends = HashMap::new();
    for kind in unique_kinds {
        let backend = config.backend(kind)?.clone();

        let resolved_cats = if let Some(ref client_ref) = client {
            println! { "Resolving Lunch Money category names for provider {}...", kind };
            let query = lunch_money::categories::query_params::CategoryQuery::builder()
                .format("flattened".to_string())
                .build();
            let lm_categories = client_ref
                .fetch_categories(&query)
                .await
                .context("Failed to fetch Lunch Money categories")?;

            let mut map = HashMap::new();
            for (payslip_item, lm_cat_name) in &backend.mapping {
                let resolved = find_category(&lm_categories, lm_cat_name).with_context(|| {
                    format!(
                        "Failed to resolve mapping for payslip item '{}' under provider {}",
                        payslip_item, kind
                    )
                })?;
                map.insert(payslip_item.clone(), resolved);
            }
            map
        } else {
            let mut map = HashMap::new();
            for (payslip_item, lm_cat_name) in &backend.mapping {
                map.insert(payslip_item.clone(), (lm_cat_name.clone(), CategoryId(0)));
            }
            map
        };

        let resolved_net_zero_acct = if let Some(ref client_ref) = client {
            let acct_name = &backend.net_zero_account;
            println! { "Resolving Lunch Money account name '{}' for provider {}...", acct_name, kind };
            Some(
                resolve_account(client_ref, acct_name)
                    .await?
                    .ok_or_else(|| {
                        anyhow!(
                            "Configured net-zero account '{}' does not exist in Lunch Money for provider {}.",
                            acct_name,
                            kind
                        )
                    })?,
            )
        } else {
            None
        };

        let resolved_rsu_acct = if !kind.uses_rsu_reconstruction() {
            None
        } else if let Some(ref client_ref) = client {
            let acct_name = backend
                .rsu_account
                .as_ref()
                .expect("rsu_account is required for RSU-reconstruction backends");
            println! { "Resolving Lunch Money RSU account name '{}' for provider {}...", acct_name, kind };
            Some(
                resolve_account(client_ref, acct_name)
                    .await?
                    .ok_or_else(|| {
                        anyhow!(
                            "Configured RSU account '{}' does not exist in Lunch Money for provider {}.",
                            acct_name,
                            kind
                        )
                    })?,
            )
        } else {
            None
        };

        resolved_backends.insert(
            kind,
            ResolvedBackend {
                backend,
                resolved_cats,
                resolved_net_zero_acct,
                resolved_rsu_acct,
            },
        );
    }

    preflight_validate(&pages_to_process, &resolved_backends)?;

    let resolved_tag_id = if let Some(ref tag_name) = config.global.tag {
        if let Some(ref client_ref) = client {
            println! { "Resolving Lunch Money tag '{}'...", tag_name };
            let lm_tags = client_ref
                .fetch_tags()
                .await
                .context("Failed to fetch Lunch Money tags")?;

            let existing_tag = lm_tags
                .iter()
                .find(|t| t.name.eq_ignore_ascii_case(tag_name));

            if let Some(tag) = existing_tag {
                Some(tag.id)
            } else if cli.dry_run {
                println! { "Tag '{}' does not exist. Would create it in Lunch Money.", tag_name };
                None
            } else {
                println! { "Tag '{}' does not exist. Creating it in Lunch Money...", tag_name };
                let new_tag = client_ref
                    .create_tag(tag_name, Some("Created by Lunch Money Payslip Importer"))
                    .await
                    .context("Failed to create missing tag")?;
                Some(new_tag.id)
            }
        } else {
            None
        }
    } else {
        None
    };

    let mut matched_tx_ids = HashSet::new();

    let mut min_date = pages_to_process[0].page.check_date;
    let mut max_date = pages_to_process[0].page.check_date;
    for ptp in &pages_to_process {
        let d = ptp.page.check_date;
        if d < min_date {
            min_date = d;
        }
        if d > max_date {
            max_date = d;
        }
    }

    let start_date = min_date
        .checked_sub(jiff::Span::new().days(3))
        .map_err(|e| anyhow!("Failed to subtract days: {}", e))?;
    let end_date = max_date
        .checked_add(jiff::Span::new().days(3))
        .map_err(|e| anyhow!("Failed to add days: {}", e))?;

    let mut unique_net_zero_accounts = HashSet::new();
    let mut unique_rsu_accounts = HashSet::new();
    for rb in resolved_backends.values() {
        if let Some(acct) = rb.resolved_net_zero_acct {
            unique_net_zero_accounts.insert(acct);
        }
        if let Some(acct) = rb.resolved_rsu_acct {
            unique_rsu_accounts.insert(acct);
        }
    }

    let mut checking_txs = Vec::new();
    if let Some(ref client_ref) = client {
        for checking_acct in &unique_net_zero_accounts {
            let query = match checking_acct {
                ResolvedAccount::Plaid(id) => TransactionQuery::builder()
                    .start_date(start_date.to_string())
                    .end_date(end_date.to_string())
                    .limit(1000)
                    .plaid_account_id(*id)
                    .maybe_include_split_parents(if cli.dry_run { Some(true) } else { None })
                    .build(),
                ResolvedAccount::Manual(id) => TransactionQuery::builder()
                    .start_date(start_date.to_string())
                    .end_date(end_date.to_string())
                    .limit(1000)
                    .manual_account_id(*id)
                    .maybe_include_split_parents(if cli.dry_run { Some(true) } else { None })
                    .build(),
            };

            let acct_name = resolved_backends
                .values()
                .find(|rb| rb.resolved_net_zero_acct == Some(*checking_acct))
                .map(|rb| rb.backend.net_zero_account.as_str())
                .unwrap_or("unknown");

            println! { "Fetching transactions for net-zero account '{}' between {} and {}...", acct_name, start_date, end_date };
            let tx_response = client_ref
                .fetch_transactions::<serde_json::Value, String>(&query)
                .await
                .context("Failed to fetch checking transactions")?;
            println! { "Fetched {} checking transactions.", tx_response.transactions.len() };
            checking_txs.extend(tx_response.transactions);
        }
    }

    let mut rsu_txs = Vec::new();
    if let Some(ref client_ref) = client {
        for rsu_acct in &unique_rsu_accounts {
            let query = match rsu_acct {
                ResolvedAccount::Plaid(id) => TransactionQuery::builder()
                    .start_date(start_date.to_string())
                    .end_date(end_date.to_string())
                    .limit(1000)
                    .plaid_account_id(*id)
                    .maybe_include_split_parents(if cli.dry_run { Some(true) } else { None })
                    .build(),
                ResolvedAccount::Manual(id) => TransactionQuery::builder()
                    .start_date(start_date.to_string())
                    .end_date(end_date.to_string())
                    .limit(1000)
                    .manual_account_id(*id)
                    .maybe_include_split_parents(if cli.dry_run { Some(true) } else { None })
                    .build(),
            };

            let acct_name = resolved_backends
                .values()
                .find(|rb| rb.resolved_rsu_acct == Some(*rsu_acct))
                .and_then(|rb| rb.backend.rsu_account.as_deref())
                .unwrap_or("unknown");

            println! { "Fetching transactions for RSU account '{}' between {} and {}...", acct_name, start_date, end_date };
            let tx_response = client_ref
                .fetch_transactions::<serde_json::Value, String>(&query)
                .await
                .context("Failed to fetch RSU transactions")?;
            println! { "Fetched {} RSU transactions.", tx_response.transactions.len() };
            rsu_txs.extend(tx_response.transactions);
        }
    }

    for ptp in &pages_to_process {
        let payslip = &ptp.page;
        let kind = ptp.kind;
        let rb = resolved_backends
            .get(&kind)
            .expect("every page's kind was resolved in the unique_kinds pass above");
        let backend = &rb.backend;
        let resolved_cats = &rb.resolved_cats;
        let resolved_net_zero_acct = &rb.resolved_net_zero_acct;
        let resolved_rsu_acct = &rb.resolved_rsu_acct;

        let check_date_str = payslip.check_date.to_string();
        let net_pay = payslip.net_pay;
        let pdf_filename = ptp
            .pdf_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();

        println! {
            "\n{STYLE_HEADER}Matching payslip {} Page {}: Check Date = {}, Net Pay = {}{STYLE_HEADER:#}",
            pdf_filename, payslip.page_num, check_date_str, net_pay
        };

        // Detect if this is an RSU vest page. Only Workday represents RSU vests
        // as separate $0 paychecks needing gross-minus-taxes reconstruction;
        // Microsoft folds stock comp inline as offsetting line items that
        // already reconcile to net pay, so it never takes the RSU path.
        let rsu_earn = if kind.uses_rsu_reconstruction() {
            rsu_vest_earning(payslip)
        } else {
            None
        };
        let is_rsu_vest = rsu_earn.is_some();

        if is_rsu_vest {
            let rsu_earn = rsu_earn.unwrap();
            let rsu_amount = rsu_earn
                .values
                .get("Amount")
                .copied()
                .unwrap_or(Decimal::ZERO);

            // Structural invariants for this page were already verified in
            // pre-flight (see preflight_validate / check_rsu_structure), which
            // runs before any mutation, so we can reconstruct safely here.

            // Calculate total taxes and gather tax split components
            let mut tax_components = Vec::new();
            for tax in &payslip.employee_taxes {
                let amount = tax.values.get("Amount").copied().unwrap_or(Decimal::ZERO);
                if !amount.is_zero() {
                    let (cat_name, cat_id) = map_category(&tax.description, resolved_cats)?;
                    tax_components.push(SplitComponent {
                        description: format!("{} - {}", backend.payslip_payee, tax.description),
                        amount,
                        category_id: cat_id,
                        category_name: cat_name,
                    });
                }
            }

            let total_taxes: Decimal = tax_components.iter().map(|c| c.amount).sum();
            let net_value = rsu_amount - total_taxes;
            let parent_amount = -net_value; // In Lunch Money, credit is negative

            let (rsu_cat_name, rsu_cat_id) = map_category(&rsu_earn.description, resolved_cats)?;
            let mut rsu_components = vec![SplitComponent {
                description: format!("{} - {}", backend.payslip_payee, rsu_earn.description),
                amount: -rsu_amount,
                category_id: rsu_cat_id,
                category_name: rsu_cat_name,
            }];
            rsu_components.extend(tax_components);

            // Match RSU vest to auto-imported Plaid transaction(s)
            let matched_plaid_rsu_txs = if let Some(rsu_acct) = resolved_rsu_acct {
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
                        // Respect cross-page dedup: a Plaid RSU transaction
                        // already claimed by an earlier page must not be matched
                        // again (overlapping ±3-day windows across pages/files
                        // would otherwise reference the same companion tx twice).
                        if matched_tx_ids.contains(&tx.id) {
                            return false;
                        }
                        let matches_acct = match rsu_acct {
                            ResolvedAccount::Plaid(id) => tx.plaid_account_id == Some(*id),
                            ResolvedAccount::Manual(id) => tx.manual_account_id == Some(*id),
                        };
                        matches_acct
                            && tx.date >= start_window
                            && tx.date <= end_window
                            && tx.amount.is_zero()
                            && tx.payee.eq_ignore_ascii_case(
                                backend.rsu_payee_match.as_deref().unwrap_or_default(),
                            )
                    })
                    .cloned()
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            };

            let mut synthetic_tx_date = payslip.check_date;
            if !matched_plaid_rsu_txs.is_empty() {
                synthetic_tx_date = matched_plaid_rsu_txs[0].date;
                // Claim these transactions so a later page cannot match the same
                // companion tx (mirrors the net-zero path's dedup).
                for tx in &matched_plaid_rsu_txs {
                    matched_tx_ids.insert(tx.id);
                }
                let matched_ids: Vec<String> = matched_plaid_rsu_txs
                    .iter()
                    .map(|tx| tx.id.to_string())
                    .collect();
                println! {
                    "  {STYLE_SUCCESS}✅ Matched RSU vest to auto-imported Plaid transaction(s): {}{STYLE_SUCCESS:#}",
                    matched_ids.join(", ")
                };
                for tx in &matched_plaid_rsu_txs {
                    if tx.is_split_parent.unwrap_or(false) {
                        println! {
                            "  {STYLE_WARNING}⚠️ Warning: Matched RSU transaction ID {} has already been split.{STYLE_WARNING:#}",
                            tx.id
                        };
                    }
                }
            }

            if cli.dry_run || cli.interactive {
                let plan_tag = if cli.dry_run { "[Dry Run] " } else { "" };
                println! {
                    "  {STYLE_INFO}ℹ️ {plan_tag}RSU Vest Detected. Creating synthetic transaction in account '{}'.{STYLE_INFO:#}",
                    backend.rsu_account.as_deref().unwrap_or_default()
                };
                if !matched_plaid_rsu_txs.is_empty() {
                    let matched_ids: Vec<String> = matched_plaid_rsu_txs
                        .iter()
                        .map(|tx| format!("#{}", tx.id))
                        .collect();
                    println! {
                        "  {STYLE_INFO}ℹ️ {plan_tag}Aligned date to matched Plaid transaction date ({}) and will reference companion Plaid transaction(s) in notes: {}{STYLE_INFO:#}",
                        synthetic_tx_date, matched_ids.join(", ")
                    };
                }
                println! {
                    "\n  {STYLE_HEADER}Plan: Create transaction (Date: {}, Amount: {}, Payee: {}){STYLE_HEADER:#}",
                    synthetic_tx_date, parent_amount, backend.payslip_payee
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
            }

            // A plain dry run (no interactive prompt) just prints the plan and
            // moves on. With --interactive we still run the prompt so the flow
            // can be exercised, but a confirmed "Yes" is a no-op under dry run.
            if cli.dry_run && !cli.interactive {
                continue;
            }

            if cli.interactive {
                match confirm_operation() {
                    Decision::Yes => {
                        if cli.dry_run {
                            println! { "  {STYLE_INFO}ℹ️ [Dry Run] Confirmed — no changes made.{STYLE_INFO:#}" };
                            continue;
                        }
                    }
                    Decision::Skip => {
                        println! { "  {STYLE_WARNING}⏭️  Skipped this operation.{STYLE_WARNING:#}" };
                        continue;
                    }
                    Decision::Stop => {
                        println! { "  {STYLE_WARNING}🛑 Stopping; no further operations will be performed.{STYLE_WARNING:#}" };
                        return Ok(());
                    }
                }
            }

            {
                if let Some(ref client_ref) = client {
                    if let Some(rsu_acct) = resolved_rsu_acct {
                        println! {
                            "  RSU Vest Detected. Creating synthetic transaction in RSU account '{}'...{STYLE_INFO:#}",
                            backend.rsu_account.as_deref().unwrap_or_default()
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
                            backend.payslip_payee.clone(),
                            notes,
                            rsu_cat_id,
                            rsu_acct,
                            resolved_tag_id,
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

                        let mut child_txs: Vec<SplitTransactionObject> = rsu_components
                            .iter()
                            .map(|comp| {
                                SplitTransactionObject::builder()
                                    .amount(comp.amount)
                                    .maybe_payee(Some(comp.description.clone()))
                                    .maybe_category_id(Some(comp.category_id))
                                    .maybe_notes(Some(" ".to_string()))
                                    .maybe_tag_ids(resolved_tag_id.map(|id| vec![id]))
                                    .build()
                            })
                            .collect();

                        optimize_split_ordering(&mut child_txs, new_tx.amount);

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
        let mut tx_payee = backend.payslip_payee.clone();

        // In interactive mode, creating the synthetic zero-pay parent is
        // deferred until after the split plan has been printed and confirmed,
        // so the user reviews the whole operation before any new transaction is
        // made. When set, holds the resolved account and parent category to
        // create with once the user approves at the confirmation gate below.
        let mut pending_zero_pay: Option<(ResolvedAccount, CategoryId)> = None;

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

                // Verify the transaction belongs to the correct net-zero account
                let matches_acct = match resolved_net_zero_acct {
                    Some(ResolvedAccount::Plaid(id)) => tx.plaid_account_id == Some(*id),
                    Some(ResolvedAccount::Manual(id)) => tx.manual_account_id == Some(*id),
                    None => false,
                };
                if !matches_acct {
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
                if tx.is_split_parent.unwrap_or(false) {
                    println! {
                        "  {STYLE_WARNING}⚠️ Warning: Transaction ID {} has already been split.{STYLE_WARNING:#}",
                        tx.id
                    };
                }

                tx_id = tx.id;
                tx_date = tx.date;
                tx_amount = tx.amount;
                tx_payee = tx.payee.clone();
            } else {
                // If no matching transaction is found and this is a net-zero check, create a synthetic transaction
                if payslip.net_pay.is_zero() {
                    let account_name = &backend.net_zero_account;

                    if cli.dry_run {
                        println! {
                            "  {STYLE_INFO}ℹ️ [Dry Run] No match found. Would create synthetic $0.00 transaction for Page {} in account '{}'.{STYLE_INFO:#}",
                            payslip.page_num, account_name
                        };
                        tx_id = TransactionId(0);
                        tx_date = payslip.check_date;
                        tx_amount = Decimal::ZERO;
                        tx_payee = backend.payslip_payee.clone();
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

                        // Categorize the synthetic parent with the page's
                        // primary earning category (largest-magnitude current
                        // earning), mirroring the RSU helper so the parent is
                        // never left uncategorized if a child split fails.
                        let parent_cat_id = payslip
                            .earnings
                            .iter()
                            .filter(|e| {
                                !e.values
                                    .get("Amount")
                                    .copied()
                                    .unwrap_or(Decimal::ZERO)
                                    .is_zero()
                            })
                            .max_by_key(|e| {
                                e.values
                                    .get("Amount")
                                    .copied()
                                    .unwrap_or(Decimal::ZERO)
                                    .abs()
                            })
                            .and_then(|e| {
                                lookup_category(&e.description, resolved_cats).map(|(_, id)| id)
                            })
                            .unwrap_or(CategoryId(0));

                        if cli.interactive {
                            // Defer creation until the split plan is printed and
                            // confirmed below, so the user reviews the whole
                            // operation before any transaction is created.
                            println! {
                                "  {STYLE_INFO}ℹ️ No match found. Would create synthetic $0.00 transaction for Page {} in account '{}' (pending confirmation).{STYLE_INFO:#}",
                                payslip.page_num, account_name
                            };
                            pending_zero_pay = Some((*acct, parent_cat_id));
                            tx_id = TransactionId(0);
                            tx_date = payslip.check_date;
                            tx_amount = Decimal::ZERO;
                            tx_payee = backend.payslip_payee.clone();
                        } else {
                            println! {
                                "  {STYLE_INFO}ℹ️ No match found. Creating synthetic $0.00 transaction for Page {} in account '{}'...{STYLE_INFO:#}",
                                payslip.page_num, account_name
                            };

                            let insert_tx = insert_transaction_for_zero_pay(
                                payslip.check_date,
                                acct,
                                backend.payslip_payee.clone(),
                                parent_cat_id,
                                resolved_tag_id,
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

        // Imputed income is provider-specific: only backends that list non-cash
        // items as one-sided earnings add-backs need an offset injected for the
        // paycheck to reconcile (see PayslipKind::injects_imputed_offsets). The
        // detection rule (Workday's leading `*`, plus configured unmarked
        // descriptions) lives in the backend; here we just dispatch through the
        // detected kind, which returns false for providers that reconcile on
        // their own (Microsoft, ADP-Microsoft).
        let imputed_descriptions = &backend.imputed_income.descriptions;
        let is_imputed_income =
            |desc: &str| -> bool { kind.is_imputed_income(desc, imputed_descriptions) };

        // 1. Earnings (credits - negative amount in Lunch Money)
        for earn in &payslip.earnings {
            let amount = earn.values.get("Amount").copied().unwrap_or(Decimal::ZERO);
            if !amount.is_zero() {
                let (cat_name, cat_id) = map_category(&earn.description, resolved_cats)?;
                components.push(SplitComponent {
                    description: format!("{} - {}", backend.payslip_payee, earn.description),
                    amount: -amount,
                    category_id: cat_id,
                    category_name: cat_name.clone(),
                });

                if is_imputed_income(&earn.description) {
                    components.push(SplitComponent {
                        description: format!(
                            "{} - {} Offset",
                            backend.payslip_payee, earn.description
                        ),
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
                let (cat_name, cat_id) = map_category(&ded.description, resolved_cats)?;
                components.push(SplitComponent {
                    description: format!("{} - {}", backend.payslip_payee, ded.description),
                    amount,
                    category_id: cat_id,
                    category_name: cat_name.clone(),
                });

                if is_imputed_income(&ded.description) {
                    components.push(SplitComponent {
                        description: format!(
                            "{} - {} Offset",
                            backend.payslip_payee, ded.description
                        ),
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
                let (cat_name, cat_id) = map_category(&tax.description, resolved_cats)?;
                components.push(SplitComponent {
                    description: format!("{} - {}", backend.payslip_payee, tax.description),
                    amount,
                    category_id: cat_id,
                    category_name: cat_name.clone(),
                });

                if is_imputed_income(&tax.description) {
                    components.push(SplitComponent {
                        description: format!(
                            "{} - {} Offset",
                            backend.payslip_payee, tax.description
                        ),
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
                let (cat_name, cat_id) = map_category(&ded.description, resolved_cats)?;
                components.push(SplitComponent {
                    description: format!("{} - {}", backend.payslip_payee, ded.description),
                    amount,
                    category_id: cat_id,
                    category_name: cat_name.clone(),
                });

                if is_imputed_income(&ded.description) {
                    components.push(SplitComponent {
                        description: format!(
                            "{} - {} Offset",
                            backend.payslip_payee, ded.description
                        ),
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

        let mut child_txs: Vec<SplitTransactionObject> = components
            .iter()
            .map(|comp| {
                SplitTransactionObject::builder()
                    .amount(comp.amount)
                    .maybe_payee(Some(comp.description.clone()))
                    .maybe_category_id(Some(comp.category_id))
                    .maybe_notes(Some(" ".to_string()))
                    .maybe_tag_ids(resolved_tag_id.map(|id| vec![id]))
                    .build()
            })
            .collect();

        optimize_split_ordering(&mut child_txs, tx_amount);

        let payload = SplitTransactionPayload {
            child_transactions: child_txs,
        };

        if cli.dry_run || cli.interactive {
            if pending_zero_pay.is_some() {
                println! {
                    "\n  {STYLE_HEADER}Plan: Create synthetic $0.00 transaction (Date: {}, Payee: {}), then split it{STYLE_HEADER:#}",
                    tx_date, tx_payee
                };
            } else {
                println! {
                    "\n  {STYLE_HEADER}Plan: Split transaction ID {} (Date: {}, Amount: {}, Payee: {}){STYLE_HEADER:#}",
                    tx_id, tx_date, tx_amount, tx_payee
                };
            }
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
        }

        // A plain dry run (no interactive prompt) just prints the plan and
        // moves on. With --interactive we still run the prompt so the flow
        // can be exercised, but a confirmed "Yes" is a no-op under dry run.
        if cli.dry_run && !cli.interactive {
            continue;
        }

        if cli.interactive {
            match confirm_operation() {
                Decision::Yes => {
                    if cli.dry_run {
                        println! { "  {STYLE_INFO}ℹ️ [Dry Run] Confirmed — no changes made.{STYLE_INFO:#}" };
                        continue;
                    }
                }
                Decision::Skip => {
                    println! { "  {STYLE_WARNING}⏭️  Skipped this operation.{STYLE_WARNING:#}" };
                    continue;
                }
                Decision::Stop => {
                    println! { "  {STYLE_WARNING}🛑 Stopping; no further operations will be performed.{STYLE_WARNING:#}" };
                    return Ok(());
                }
            }
        }

        {
            // Materialize a deferred zero-pay parent (interactive mode defers
            // its creation until the operation is confirmed). On any failure,
            // skip this page rather than splitting a transaction that does not
            // exist.
            if let Some((acct, parent_cat_id)) = pending_zero_pay {
                if let Some(ref client_ref) = client {
                    println! {
                        "  {STYLE_INFO}ℹ️ Creating synthetic $0.00 transaction for Page {} in account '{}'...{STYLE_INFO:#}",
                        payslip.page_num, &backend.net_zero_account
                    };
                    let insert_tx = insert_transaction_for_zero_pay(
                        payslip.check_date,
                        &acct,
                        backend.payslip_payee.clone(),
                        parent_cat_id,
                        resolved_tag_id,
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

                    // Only tx_id is consumed by the split below; the plan has
                    // already been printed, so the other fields need no update.
                    tx_id = new_tx.id;
                }
            }

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

/// Returns the earnings row representing a non-zero restricted-stock vest on
/// this page, if any. Used to classify a page as an RSU vest both in pre-flight
/// validation and in the import loop, so the two never diverge.
fn rsu_vest_earning(payslip: &crate::payslip::ParsedPage) -> Option<&crate::payslip::RowData> {
    payslip
        .earnings
        .iter()
        .find(|earn| earn.description.to_lowercase().contains("restricted stock"))
        .filter(|earn| {
            !earn
                .values
                .get("Amount")
                .copied()
                .unwrap_or(Decimal::ZERO)
                .is_zero()
        })
}

/// Verify the structural invariants an RSU vest page must satisfy before we
/// reconstruct it as gross comp minus taxes. Pushes a human-readable problem
/// onto `problems` for each violation rather than bailing, so pre-flight can
/// report every issue across the whole PDF at once.
fn check_rsu_structure(
    payslip: &crate::payslip::ParsedPage,
    rsu_earn: &crate::payslip::RowData,
    problems: &mut Vec<String>,
) {
    // The RSU path short-circuits the regular earnings loop, so any other
    // earning with a non-zero current amount would be silently dropped and its
    // taxes misattributed to the vest.
    let other_earnings: Vec<&str> = payslip
        .earnings
        .iter()
        .filter(|e| !std::ptr::eq(*e, rsu_earn))
        .filter(|e| {
            !e.values
                .get("Amount")
                .copied()
                .unwrap_or(Decimal::ZERO)
                .is_zero()
        })
        .map(|e| e.description.as_str())
        .collect();
    if !other_earnings.is_empty() {
        problems.push(format!(
            "Page {}: RSU vest page also has non-zero earnings [{}]. Combined vest+earnings runs are not supported (the RSU path would drop these and misattribute taxes). Split this page manually.",
            payslip.page_num,
            other_earnings.join(", ")
        ));
    }

    // RSU vests settle to $0 cash: employee taxes are exactly cancelled by the
    // RSU Tax Offset post-tax deduction. The reconstruction excludes deductions,
    // so confirm the deductions genuinely net out the taxes.
    let taxes_total: Decimal = payslip
        .employee_taxes
        .iter()
        .map(|t| t.values.get("Amount").copied().unwrap_or(Decimal::ZERO))
        .sum();
    let deductions_total: Decimal = payslip
        .pre_tax_deductions
        .iter()
        .chain(payslip.post_tax_deductions.iter())
        .map(|d| d.values.get("Amount").copied().unwrap_or(Decimal::ZERO))
        .sum();
    let settlement = taxes_total + deductions_total;
    if settlement.abs() >= Decimal::new(1, 2) {
        problems.push(format!(
            "Page {}: RSU vest does not settle to $0 — employee taxes ({}) and deductions ({}) do not cancel (residual {}). Expected the RSU Tax Offset to zero out withholdings; refusing to import a mis-parsed vest.",
            payslip.page_num, taxes_total, deductions_total, settlement
        ));
    }

    // Independently confirm the summary row reports $0 net pay (this figure is
    // not used by the reconstruction, so it is a real check).
    if !payslip.net_pay.is_zero() {
        problems.push(format!(
            "Page {}: RSU vest page reports non-zero net pay ({}). Expected a $0 settlement; refusing to import.",
            payslip.page_num, payslip.net_pay
        ));
    }
}

/// Validate every page before any Lunch Money mutation occurs, so a problem on
/// a late page can never leave an earlier page half-imported (audit #5). Every
/// distinct line item must resolve to a category, and every RSU vest page must
/// satisfy its structural invariants. All problems are collected and reported
/// together rather than failing on the first one.
fn preflight_validate(
    pages_to_process: &[PageToProcess],
    resolved_backends: &HashMap<crate::payslip::PayslipKind, ResolvedBackend>,
) -> Result<()> {
    let mut problems = Vec::new();
    let mut unmapped: std::collections::BTreeSet<(crate::payslip::PayslipKind, String)> =
        std::collections::BTreeSet::new();

    for ptp in pages_to_process {
        let payslip = &ptp.page;
        let kind = ptp.kind;
        let rb = resolved_backends
            .get(&kind)
            .ok_or_else(|| anyhow!("Backend info not found for kind {:?}", kind))?;
        let resolved_cats = &rb.resolved_cats;

        // Only providers that encode RSU vests as separate $0 paychecks
        // (Workday) take the gross-minus-taxes reconstruction path; others
        // fold stock comp inline, so the RSU structural check never applies.
        let rsu_earn = if kind.uses_rsu_reconstruction() {
            rsu_vest_earning(payslip)
        } else {
            None
        };
        if let Some(rsu_earn) = rsu_earn {
            check_rsu_structure(payslip, rsu_earn, &mut problems);
        }

        // Every non-zero line item that would become a split component must
        // resolve to a category. On an RSU page only the vest line and the
        // employee taxes are emitted; on a regular page all four sections are.
        let mut items: Vec<&crate::payslip::RowData> = Vec::new();
        if let Some(rsu_earn) = rsu_earn {
            items.push(rsu_earn);
            items.extend(payslip.employee_taxes.iter());
        } else {
            items.extend(payslip.earnings.iter());
            items.extend(payslip.pre_tax_deductions.iter());
            items.extend(payslip.employee_taxes.iter());
            items.extend(payslip.post_tax_deductions.iter());
        }

        for item in items {
            let amount = item.values.get("Amount").copied().unwrap_or(Decimal::ZERO);
            if amount.is_zero() {
                continue;
            }
            if lookup_category(&item.description, resolved_cats).is_none() {
                unmapped.insert((kind, item.description.clone()));
            }
        }
    }

    for (kind, desc) in &unmapped {
        problems.push(format!(
            "No Lunch Money category mapping found for payslip item '{desc}' (provider: {kind}). Add it to the [backends.{kind}.mapping] section of lm_payslip_importer.toml."
        ));
    }

    if !problems.is_empty() {
        let mut msg = format!(
            "Pre-flight validation found {} problem(s); no transactions were imported:\n",
            problems.len()
        );
        for p in &problems {
            msg.push_str(&format!("  • {p}\n"));
        }
        anyhow::bail!("{}", msg.trim_end());
    }

    Ok(())
}

fn lookup_category(
    desc: &str,
    resolved_mapping: &HashMap<String, (String, CategoryId)>,
) -> Option<(String, CategoryId)> {
    if let Some(val) = resolved_mapping.get(desc) {
        return Some(val.clone());
    }

    // Try case-insensitive lookup as fallback
    let desc_lower = desc.to_lowercase();
    for (k, v) in resolved_mapping {
        if k.to_lowercase() == desc_lower {
            return Some(v.clone());
        }
    }

    None
}

fn map_category(
    desc: &str,
    resolved_mapping: &HashMap<String, (String, CategoryId)>,
) -> Result<(String, CategoryId)> {
    lookup_category(desc, resolved_mapping).ok_or_else(|| {
        anyhow!(
            "No Lunch Money category mapping found for payslip item '{}'. Please add it to the [mapping] section of lm_payslip_importer.toml.",
            desc
        )
    })
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
    tag_id: Option<TagId>,
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
        .maybe_tag_ids(tag_id.map(|id| vec![id]))
        .build())
}

fn insert_transaction_for_zero_pay(
    date: Date,
    resolved_acct: &ResolvedAccount,
    payee: String,
    category_id: CategoryId,
    tag_id: Option<TagId>,
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
        .maybe_category_id(Some(category_id))
        .maybe_plaid_account_id(plaid_id)
        .maybe_manual_account_id(manual_id)
        .maybe_tag_ids(tag_id.map(|id| vec![id]))
        .build())
}

/// Reorders child split transactions in-place so that their f64 floating-point sum
/// matches the parent target amount exactly. This works around strict float validations
/// in Lunch Money's API which do not account for IEEE-754 precision issues under arbitrary addition order.
fn optimize_split_ordering(child_txs: &mut [SplitTransactionObject], target_decimal: Decimal) {
    use rust_decimal::prelude::ToPrimitive;

    let target = target_decimal.to_f64().unwrap_or(0.0);

    // Check if the current ordering already sums to target
    let current_sum: f64 = child_txs
        .iter()
        .map(|c| c.amount.to_f64().unwrap_or(0.0))
        .sum();
    if current_sum == target {
        return;
    }

    let mut seed = 0x123456789abcdef0u64;

    for _ in 0..100_000 {
        // xorshift64 shuffle
        let n = child_txs.len();
        for i in (1..n).rev() {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let j = (seed as usize) % (i + 1);
            child_txs.swap(i, j);
        }

        let sum: f64 = child_txs
            .iter()
            .map(|c| c.amount.to_f64().unwrap_or(0.0))
            .sum();
        if sum == target {
            return;
        }
    }
}
