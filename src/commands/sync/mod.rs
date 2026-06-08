mod diff;
mod execute;
mod report;

use crate::api::ExpensesQuery;
use crate::api::TransactionQuery;
use crate::api::lunch_money::schema::InsertObject;
use crate::api::lunch_money::schema::Transaction;
use crate::api::lunch_money::schema::UpdateObject;
use crate::style::*;
use anstream::println;
use anyhow::Context;
use std::collections::HashMap;

pub use diff::DiffTransactionsArgs;
pub use diff::diff_transactions;
pub use execute::ApplySyncPlanArgs;
pub use execute::apply_sync_plan;
pub use report::PrintAndLogSyncPlanArgs;
pub use report::print_and_log_sync_plan;

pub struct SyncPlan {
    pub inserts: Vec<InsertObject>,
    pub updates: Vec<UpdateObject>,
    pub deletes: Vec<Transaction>,
    pub tags_to_create: Vec<String>,
}

impl SyncPlan {
    #[expect(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.inserts.is_empty()
            && self.updates.is_empty()
            && self.deletes.is_empty()
            && self.tags_to_create.is_empty()
    }
}

pub enum SyncMode {
    Window {
        window: std::time::Duration,
        from: Option<jiff::civil::Date>,
        no_groups: bool,
    },
    Group {
        group_query: String,
        force_category: Option<String>,
    },
}

pub struct SyncOptions {
    pub dry_run: bool,
    pub tag: Option<String>,
    pub no_loan_tag: bool,
    pub no_ignore: bool,
    pub csv_path: Option<std::path::PathBuf>,
    pub mode: SyncMode,
}

pub struct SyncOrchestrator<'a> {
    pub ctx: &'a crate::AppContext,
}

impl<'a> SyncOrchestrator<'a> {
    pub fn new(ctx: &'a crate::AppContext) -> Self {
        Self { ctx }
    }

    pub async fn execute(&self, opts: SyncOptions) -> anyhow::Result<()> {
        let sw_client = &self.ctx.splitwise;
        let lm_client = &self.ctx.lunch_money;

        // Fetch groups
        let groups = sw_client.fetch_groups().await?;
        let group_map: HashMap<u64, String> =
            groups.iter().map(|g| (g.id, g.name.clone())).collect();

        // Mode specific print headers & fetch expenses
        let dry_run_suffix = if opts.dry_run {
            format!(" {STYLE_WARNING}[DRY RUN]{STYLE_WARNING:#}")
        } else {
            "".to_string()
        };
        println! {};

        let expenses = match &opts.mode {
            SyncMode::Window {
                window,
                from,
                no_groups,
            } => {
                let window_duration = jiff::SignedDuration::try_from(*window)
                    .context("window duration is too large")?;

                let super::WindowBounds {
                    start: start_window_str,
                    end: end_window_str,
                } = super::calculate_window_bounds(*from, window_duration);

                let bar = "─".repeat(92);

                println! { "{STYLE_HEADER}⚡ Splitwise to Lunch Money Sync Window{}{STYLE_HEADER:#}", dry_run_suffix };
                println! { "{STYLE_DIM}{bar}{STYLE_DIM:#}" };
                println! { "{STYLE_INFO}📅 Window boundary:{STYLE_INFO:#} {} to {}", start_window_str, end_window_str };
                if *no_groups {
                    println! { "{STYLE_INFO}🚫 Filter:{STYLE_INFO:#} Non-group expenses only" };
                }
                println! {};

                println! { "  {STYLE_DIM}Fetching Splitwise groups and expenses...{STYLE_DIM:#}" };

                let mut txs = sw_client
                    .fetch_expenses(&ExpensesQuery {
                        dated_after: Some(start_window_str),
                        dated_before: from.map(|f| format!("{}T23:59:59Z", f)),
                        limit: Some(0),
                        ..Default::default()
                    })
                    .await?;

                if *no_groups {
                    txs.retain(|e| e.parsed.group_id.is_none());
                }
                txs
            }
            SyncMode::Group { group_query, .. } => {
                let target_group = super::resolve_group(&groups, group_query)?;

                if self
                    .ctx
                    .config
                    .splitwise
                    .is_group_ignored(target_group.id, Some(&target_group.name))
                    && !opts.no_ignore
                {
                    anyhow::bail!(
                        "Group {} is marked as ignored in configuration. To force synchronization for this group, use the --no-ignore flag.",
                        target_group.id
                    );
                }

                println! { "{STYLE_HEADER}⚡ Splitwise to Lunch Money Sync Group{}{STYLE_HEADER:#}", dry_run_suffix };
                println! { "{STYLE_DIM}──────────────────────────────────────────────────{STYLE_DIM:#}" };
                println! { "{STYLE_INFO}👥 Group:{STYLE_INFO:#} {} (ID: {})", target_group.name, target_group.id };
                if target_group.id != 0 {
                    let balance_str = super::format_group_balances(
                        &target_group,
                        self.ctx.config.splitwise.user_id,
                    );
                    println! { "{STYLE_INFO}💰 Balance:{STYLE_INFO:#} {}", balance_str };
                }
                println! {};

                println! { "  {STYLE_DIM}Fetching Splitwise groups and expenses...{STYLE_DIM:#}" };
                sw_client
                    .fetch_expenses(&ExpensesQuery {
                        group_id: Some(target_group.id),
                        limit: Some(0),
                        ..Default::default()
                    })
                    .await?
            }
        };

        // Prepare helper map for printing and CSV writing
        let mut sw_expense_categories = HashMap::new();
        for expense in &expenses {
            let ext_id = crate::api::ExternalId::Splitwise(expense.parsed.id);
            let cat_info = if expense.parsed.payment {
                Some((0, "Payment".to_string()))
            } else {
                expense
                    .parsed
                    .category
                    .as_ref()
                    .map(|c| (c.id, c.name.clone()))
            };
            sw_expense_categories.insert(ext_id, cat_info);
        }

        // Verify configured manual accounts exist in Lunch Money
        let manual_accounts = lm_client.fetch_manual_accounts().await?;
        let target_accounts = crate::commands::resolve_target_accounts(
            &manual_accounts,
            &self.ctx.config.lunch_money.custom_accounts,
        );
        verify_target_accounts(&target_accounts, &manual_accounts)?;

        let ResolvedCategories {
            resolved_categories,
            mut lm_category_names,
        } = resolve_categories(lm_client, &self.ctx.config).await?;

        let force_category_id = match &opts.mode {
            SyncMode::Group {
                force_category: Some(fc),
                ..
            } => Some(resolve_force_category(lm_client, fc, &mut lm_category_names).await?),
            _ => None,
        };

        let sw_category_id_to_path =
            fetch_splitwise_categories(sw_client, &self.ctx.config).await?;

        let loan_tag_name = if opts.no_loan_tag {
            None
        } else {
            self.ctx.config.sync.loan_tag.as_deref()
        };

        // Planning step for tags (dry-run safe)
        let PlannedTags {
            tag_id,
            loan_tag_id,
            tags_to_create,
        } = plan_tags(lm_client, opts.tag.as_deref(), loan_tag_name).await?;

        // Mode specific Lunch Money transactions fetching date ranges
        let super::WindowBounds {
            start: start_date_str,
            end: end_date_str,
        } = match &opts.mode {
            SyncMode::Window { window, from, .. } => {
                let window_duration = jiff::SignedDuration::try_from(*window)
                    .context("window duration is too large")?;
                super::calculate_window_bounds(*from, window_duration)
            }
            SyncMode::Group { .. } => {
                let end_str = jiff::Timestamp::now()
                    .to_zoned(jiff::tz::TimeZone::UTC)
                    .strftime("%Y-%m-%d")
                    .to_string();
                super::WindowBounds {
                    start: "2000-01-01".to_string(),
                    end: end_str,
                }
            }
        };

        let lm_transactions = fetch_lunch_money_transactions(FetchLunchMoneyTransactionsArgs {
            lm_client,
            target_accounts: &target_accounts,
            manual_accounts: &manual_accounts,
            start_date_str: &start_date_str,
            end_date_str: &end_date_str,
        })
        .await?;

        println! { "  {STYLE_DIM}Comparing transactions...{STYLE_DIM:#}" };
        println! {};

        let mut lm_tx_categories = HashMap::new();
        for t in &lm_transactions {
            lm_tx_categories.insert(t.id, (t.external_id.clone(), t.category_id));
        }

        let mut lm_map: HashMap<crate::api::ExternalId, Transaction> = lm_transactions
            .into_iter()
            .filter_map(|t| t.external_id.clone().map(|ext_id| (ext_id, t)))
            .collect();

        let ignored_groups_exclude = match &opts.mode {
            SyncMode::Group { group_query, .. } => {
                let target_group = super::resolve_group(&groups, group_query)?;
                Some(target_group.id)
            }
            _ => None,
        };

        // Compute pure diff plan (excludes mutating actions like tag creations)
        let mut plan = diff_transactions(DiffTransactionsArgs {
            expenses,
            config: &self.ctx.config,
            target_accounts: &target_accounts,
            group_map: &group_map,
            lm_map: &mut lm_map,
            sw_category_id_to_path: &sw_category_id_to_path,
            resolved_categories: &resolved_categories,
            ignored_groups_exclude,
            bypass_ignore_groups: opts.no_ignore,
            tag_id,
            loan_tag_id,
            force_category_id,
            tags_to_create,
        })?;

        // Mode specific post-diff deletes filtering
        if let SyncMode::Group { group_query, .. } = &opts.mode {
            let target_group = super::resolve_group(&groups, group_query)?;
            let is_non_group = target_group.id == 0;
            let group_payee = format!("Splitwise - {}", target_group.name);

            for (_ext_id, t) in lm_map {
                let belongs_to_group = if is_non_group {
                    !t.payee.starts_with("Splitwise - ")
                        || t.payee == "Splitwise - Non-group"
                        || (!group_map
                            .values()
                            .any(|gn| t.payee == format!("Splitwise - {}", gn))
                            && t.payee.starts_with("Splitwise - "))
                } else {
                    t.payee == group_payee
                };

                if belongs_to_group && t.is_split_parent != Some(true) {
                    plan.deletes.push(t);
                }
            }
        }

        // Display/Reporting Stage
        print_and_log_sync_plan(PrintAndLogSyncPlanArgs {
            plan: &plan,
            dry_run: opts.dry_run,
            lm_category_names: &lm_category_names,
            sw_expense_categories: &sw_expense_categories,
            sw_category_id_to_path: &sw_category_id_to_path,
            lm_tx_categories: &lm_tx_categories,
            csv_path: opts.csv_path.as_deref(),
        })?;

        if !opts.dry_run {
            apply_sync_plan(ApplySyncPlanArgs {
                plan: &mut plan,
                lm_client,
                manual_accounts: &manual_accounts,
                target_accounts: &target_accounts,
                tag_name: opts.tag.as_deref(),
                loan_tag_name,
            })
            .await?;
        }

        Ok(())
    }
}

pub(crate) async fn run_sync_window(
    ctx: &crate::AppContext,
    sync_args: crate::cli::SyncWindowArgs,
) -> anyhow::Result<()> {
    let orchestrator = SyncOrchestrator::new(ctx);
    orchestrator
        .execute(SyncOptions {
            dry_run: sync_args.dry_run,
            tag: sync_args.tag,
            no_loan_tag: sync_args.no_loan_tag,
            no_ignore: sync_args.no_ignore,
            csv_path: sync_args.csv,
            mode: SyncMode::Window {
                window: sync_args.window,
                from: sync_args.from,
                no_groups: sync_args.no_groups,
            },
        })
        .await
}

pub(crate) async fn run_sync_group(
    ctx: &crate::AppContext,
    sync_args: crate::cli::SyncGroupArgs,
) -> anyhow::Result<()> {
    let groups = ctx.splitwise.fetch_groups().await?;
    let target_group = super::resolve_group(&groups, &sync_args.group)?;

    let csv_path = match sync_args.csv {
        Some(Some(path)) => Some(path),
        Some(None) => {
            let filename = format!("{}.csv", target_group.name);
            Some(std::path::PathBuf::from(filename))
        }
        None => None,
    };

    let orchestrator = SyncOrchestrator::new(ctx);
    orchestrator
        .execute(SyncOptions {
            dry_run: sync_args.dry_run,
            tag: sync_args.tag,
            no_loan_tag: sync_args.no_loan_tag,
            no_ignore: sync_args.no_ignore,
            csv_path,
            mode: SyncMode::Group {
                group_query: sync_args.group,
                force_category: sync_args.force_category,
            },
        })
        .await
}

fn verify_target_accounts(
    target_accounts: &HashMap<crate::api::Currency, u64>,
    manual_accounts: &[crate::api::lunch_money::schema::ManualAccount],
) -> anyhow::Result<()> {
    if target_accounts.is_empty() {
        anyhow::bail!(
            "No active manual accounts found. Please set up an active 'Splitwise <CURRENCY>' manual account (e.g. 'Splitwise USD') in Lunch Money or configure [lunch_money.custom_accounts]."
        );
    }

    for (currency, &account_id) in target_accounts {
        if !manual_accounts.iter().any(|acc| acc.id == account_id) {
            anyhow::bail!(
                "Configured manual account ID {} for currency '{}' has been deleted or does not exist in Lunch Money.",
                account_id,
                currency
            );
        }
    }
    Ok(())
}

async fn fetch_splitwise_categories(
    sw_client: &crate::api::splitwise::Client,
    config: &crate::config::Config,
) -> anyhow::Result<HashMap<u32, String>> {
    let mut sw_category_id_to_path = HashMap::new();
    if !config.categories.is_empty() {
        println! { "  {STYLE_DIM}Fetching Splitwise categories...{STYLE_DIM:#}" };
        let sw_categories = sw_client.fetch_categories().await?;
        for parent in sw_categories {
            sw_category_id_to_path.insert(parent.id, parent.name.clone());
            for sub in parent.subcategories {
                let path = format!("{}:{}", parent.name, sub.name);
                sw_category_id_to_path.insert(sub.id, path);
            }
        }
    }
    Ok(sw_category_id_to_path)
}

struct FetchLunchMoneyTransactionsArgs<'a> {
    lm_client: &'a crate::api::lunch_money::Client,
    target_accounts: &'a HashMap<crate::api::Currency, u64>,
    manual_accounts: &'a [crate::api::lunch_money::schema::ManualAccount],
    start_date_str: &'a str,
    end_date_str: &'a str,
}

async fn fetch_lunch_money_transactions(
    args: FetchLunchMoneyTransactionsArgs<'_>,
) -> anyhow::Result<Vec<Transaction>> {
    let FetchLunchMoneyTransactionsArgs {
        lm_client,
        target_accounts,
        manual_accounts,
        start_date_str,
        end_date_str,
    } = args;
    println! { "  {STYLE_DIM}Fetching Lunch Money transactions...{STYLE_DIM:#}" };
    let mut lm_transactions = Vec::new();
    for &account_id in target_accounts.values() {
        let mut txs = lm_client
            .fetch_transactions(&TransactionQuery {
                start_date: start_date_str.to_string(),
                end_date: end_date_str.to_string(),
                manual_account_id: account_id,
                limit: Some(1000),
                include_group_children: Some(true),
                include_split_parents: Some(true),
            })
            .await?;
        let is_loan = manual_accounts
            .iter()
            .find(|acc| acc.id == account_id)
            .map(|acc| acc.account_type == crate::api::lunch_money::schema::AccountType::Loan)
            .unwrap_or(false);

        if is_loan {
            for t in &mut txs {
                t.amount = -t.amount;
            }
        }
        lm_transactions.extend(txs);
    }
    Ok(lm_transactions)
}

pub(crate) struct ResolvedCategories {
    pub resolved_categories: HashMap<String, u64>,
    pub lm_category_names: HashMap<u64, String>,
}

async fn resolve_categories(
    lm_client: &crate::api::lunch_money::Client,
    config: &crate::config::Config,
) -> anyhow::Result<ResolvedCategories> {
    if config.categories.is_empty() {
        return Ok(ResolvedCategories {
            resolved_categories: HashMap::new(),
            lm_category_names: HashMap::new(),
        });
    }

    println! { "  {STYLE_DIM}Fetching Lunch Money categories...{STYLE_DIM:#}" };
    let categories = lm_client.fetch_categories(Some("flattened")).await?;

    let names: HashMap<u64, String> = categories.iter().map(|c| (c.id, c.name.clone())).collect();

    let mut resolved = HashMap::new();
    for (sw_key, lm_val) in &config.categories {
        let resolved_id = match lm_val {
            crate::config::CategoryValue::Id(id) => {
                if categories.iter().any(|c| c.id == *id && !c.archived) {
                    *id
                } else {
                    println! { "  ⚠️  {STYLE_WARNING}Warning:{STYLE_WARNING:#} Configured Lunch Money category ID {} (for Splitwise category '{}') does not exist or is archived.", id, sw_key };
                    continue;
                }
            }
            crate::config::CategoryValue::Name(name) => {
                let matches: Vec<_> = categories
                    .iter()
                    .filter(|c| c.name.eq_ignore_ascii_case(name) && !c.archived)
                    .collect();
                if matches.is_empty() {
                    println! { "  ⚠️  {STYLE_WARNING}Warning:{STYLE_WARNING:#} Configured Lunch Money category '{}' (for Splitwise category '{}') does not exist or is archived.", name, sw_key };
                    continue;
                } else if matches.len() > 1 {
                    let mut msg = format!(
                        "Multiple active Lunch Money categories found with the name '{}':\n",
                        name
                    );
                    for m in matches {
                        msg.push_str(&format!("  • ID: {} (is_group: {})\n", m.id, m.is_group));
                    }
                    msg.push_str("Please map by category ID instead to resolve ambiguity.");
                    anyhow::bail!("{}", msg);
                } else {
                    matches[0].id
                }
            }
        };

        resolved.insert(sw_key.clone(), resolved_id);
    }
    Ok(ResolvedCategories {
        resolved_categories: resolved,
        lm_category_names: names,
    })
}

async fn resolve_force_category(
    lm_client: &crate::api::lunch_money::Client,
    force_category_str: &str,
    lm_category_names: &mut HashMap<u64, String>,
) -> anyhow::Result<u64> {
    println! { "  {STYLE_DIM}Fetching Lunch Money categories to resolve forced category...{STYLE_DIM:#}" };
    let categories = lm_client.fetch_categories(Some("flattened")).await?;

    if let Ok(id) = force_category_str.parse::<u64>() {
        if let Some(c) = categories.iter().find(|c| c.id == id && !c.archived) {
            lm_category_names.insert(id, c.name.clone());
            return Ok(id);
        } else {
            anyhow::bail!(
                "Forced category ID {} does not exist or is archived in Lunch Money.",
                id
            );
        }
    }

    let matches: Vec<_> = categories
        .iter()
        .filter(|c| c.name == force_category_str && !c.archived)
        .collect();

    if matches.is_empty() {
        anyhow::bail!(
            "Forced category '{}' does not exist or is archived in Lunch Money.",
            force_category_str
        );
    } else if matches.len() > 1 {
        let mut msg = format!(
            "Multiple active Lunch Money categories found with the name '{}':\n",
            force_category_str
        );
        for m in matches {
            msg.push_str(&format!("  • ID: {} (is_group: {})\n", m.id, m.is_group));
        }
        msg.push_str("Please specify the category ID instead to resolve ambiguity.");
        anyhow::bail!("{}", msg);
    } else {
        let matched = matches[0];
        lm_category_names.insert(matched.id, matched.name.clone());
        Ok(matched.id)
    }
}

pub(crate) struct PlannedTags {
    pub tag_id: Option<u64>,
    pub loan_tag_id: Option<u64>,
    pub tags_to_create: Vec<String>,
}

async fn plan_tags(
    lm_client: &crate::api::lunch_money::Client,
    tag_name: Option<&str>,
    loan_tag_name: Option<&str>,
) -> anyhow::Result<PlannedTags> {
    let mut tag_id = None;
    let mut loan_tag_id = None;
    let mut tags_to_create = Vec::new();

    if tag_name.is_none() && loan_tag_name.is_none() {
        return Ok(PlannedTags {
            tag_id: None,
            loan_tag_id: None,
            tags_to_create,
        });
    }

    println! { "  {STYLE_DIM}Fetching Lunch Money tags...{STYLE_DIM:#}" };
    let tags = lm_client.fetch_tags().await?;

    if let Some(name) = tag_name {
        if let Some(existing_tag) = tags.iter().find(|t| t.name.eq_ignore_ascii_case(name)) {
            tag_id = Some(existing_tag.id);
        } else {
            tags_to_create.push(name.to_string());
        }
    }

    if let Some(name) = loan_tag_name {
        if Some(name) == tag_name {
            loan_tag_id = tag_id;
        } else if let Some(existing_tag) = tags.iter().find(|t| t.name.eq_ignore_ascii_case(name)) {
            loan_tag_id = Some(existing_tag.id);
        } else {
            tags_to_create.push(name.to_string());
        }
    }

    tags_to_create.sort();
    tags_to_create.dedup();

    Ok(PlannedTags {
        tag_id,
        loan_tag_id,
        tags_to_create,
    })
}
