use crate::api::ExpensesQuery;
use crate::api::LunchMoneyService;
use crate::api::SplitwiseService;
use crate::api::TransactionQuery;
use crate::api::lunch_money::schema::InsertObject;
use crate::api::lunch_money::schema::Transaction;
use crate::api::lunch_money::schema::UpdateObject;
use crate::style::*;
use anstream::println;
use anyhow::Context;
use rust_decimal::Decimal;
use std::collections::HashMap;
use tabled::Table;
use tabled::Tabled;
use tabled::settings::Style;

#[derive(Tabled)]
struct SyncRecord {
    #[tabled(rename = "Date")]
    date: String,
    #[tabled(rename = "Payee")]
    payee: String,
    #[tabled(rename = "Category (Splitwise)")]
    sw_category: String,
    #[tabled(rename = "Category (Lunch Money)")]
    lm_category: String,
    #[tabled(rename = "Amount")]
    amount: String,
    #[tabled(rename = "Notes")]
    notes: String,
}

struct ToSyncRecordArgs<'a> {
    payee: &'a str,
    amount: Decimal,
    currency: &'a crate::api::Currency,
    date: jiff::civil::Date,
    notes: &'a str,
    sw_category_name: Option<&'a str>,
    lm_category_name: Option<&'a str>,
    max_num_len: usize,
    max_currency_len: usize,
}

/// Formats a transaction sync record into a `SyncRecord`.
/// We accept the pre-calculated `max_num_len` and `max_currency_len` to format the transaction
/// amount cell with alignment, ensuring decimals and currency codes line up vertically.
fn to_sync_record(args: ToSyncRecordArgs<'_>) -> SyncRecord {
    let ToSyncRecordArgs {
        payee,
        amount,
        currency,
        date,
        notes,
        sw_category_name,
        lm_category_name,
        max_num_len,
        max_currency_len,
    } = args;
    let date_str = date.strftime("%Y-%m-%d").to_string();

    let mut clean_payee = payee.to_string();
    if clean_payee.starts_with("Splitwise - ") {
        clean_payee = clean_payee["Splitwise - ".len()..].to_string();
    }
    if clean_payee.chars().count() > 50 {
        clean_payee = clean_payee.chars().take(47).collect::<String>();
        clean_payee.push_str("...");
    }

    let sw_clean = match sw_category_name {
        Some("Uncategorized:General") => "",
        Some(other) => other,
        None => "",
    };

    let sw_is_uncategorized = matches!(sw_category_name, None | Some("Uncategorized:General"));
    let lm_clean = if sw_is_uncategorized {
        lm_category_name.unwrap_or("")
    } else {
        lm_category_name.unwrap_or("?")
    };

    let amount_style = if amount.is_sign_negative() {
        STYLE_ERROR
    } else {
        STYLE_SUCCESS
    };
    let amount_plain =
        super::format_aligned_balance(amount, currency, max_num_len, max_currency_len, false);
    let amount_colored = format!("{}{}{:#}", amount_style, amount_plain, amount_style);

    let notes_colored = if notes.trim().is_empty() {
        "".to_string()
    } else {
        format!("{}{}{:#}", STYLE_DIM, notes.trim(), STYLE_DIM)
    };

    SyncRecord {
        date: date_str,
        payee: clean_payee,
        sw_category: sw_clean.to_string(),
        lm_category: lm_clean.to_string(),
        amount: amount_colored,
        notes: notes_colored,
    }
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

pub struct SyncOrchestrator {
    pub config: crate::config::Config,
}

impl SyncOrchestrator {
    pub fn new(config: crate::config::Config) -> Self {
        Self { config }
    }

    pub async fn execute(&self, opts: SyncOptions) -> anyhow::Result<()> {
        let http_pool = reqwest::Client::new();
        let sw_client = crate::api::splitwise::Client::new(
            http_pool.clone(),
            self.config.splitwise.api_key.clone(),
        );
        let lm_client = crate::api::lunch_money::Client::new(
            http_pool,
            self.config.lunch_money.api_key.clone(),
        );

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
                println! { "{STYLE_HEADER}⚡ Splitwise to Lunch Money Sync{}{STYLE_HEADER:#}", dry_run_suffix };
                println! { "{STYLE_DIM}──────────────────────────────────────────────────{STYLE_DIM:#}" };
                let window_duration = jiff::SignedDuration::try_from(*window)
                    .context("window duration is too large")?;
                let super::WindowBounds { start, end } =
                    super::calculate_window_bounds(*from, window_duration);
                println! { "{STYLE_INFO}📅 Sync window boundary:{STYLE_INFO:#} {} to {}", start, end };
                if *no_groups {
                    println! { "{STYLE_INFO}🚫 Filter:{STYLE_INFO:#} Non-group transactions only" };
                }
                println! {};

                println! { "  {STYLE_DIM}Fetching Splitwise groups and expenses...{STYLE_DIM:#}" };
                let mut expenses = sw_client
                    .fetch_expenses(&ExpensesQuery {
                        dated_after: Some(start.clone()),
                        dated_before: if from.is_some() {
                            Some(format!("{}T23:59:59Z", end))
                        } else {
                            None
                        },
                        limit: Some(0),
                        ..Default::default()
                    })
                    .await?;
                if *no_groups {
                    expenses.retain(|e| e.group_id.is_none());
                }
                expenses
            }
            SyncMode::Group { group_query, .. } => {
                let target_group = super::resolve_group(&groups, group_query)?;
                if self
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
                    let balance_str =
                        super::format_group_balances(&target_group, self.config.splitwise.user_id);
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
            let ext_id = crate::api::ExternalId::Splitwise(expense.id);
            let cat_info = if expense.payment {
                Some((0, "Payment".to_string()))
            } else {
                expense.category.as_ref().map(|c| (c.id, c.name.clone()))
            };
            sw_expense_categories.insert(ext_id, cat_info);
        }

        // Verify configured manual accounts exist in Lunch Money
        let manual_accounts = lm_client.fetch_manual_accounts().await?;
        let target_accounts = crate::commands::resolve_target_accounts(
            &manual_accounts,
            &self.config.lunch_money.custom_accounts,
        );
        verify_target_accounts(&target_accounts, &manual_accounts)?;

        let ResolvedCategories {
            resolved_categories,
            mut lm_category_names,
        } = resolve_categories(&lm_client, &self.config).await?;

        let force_category_id = match &opts.mode {
            SyncMode::Group {
                force_category: Some(fc),
                ..
            } => Some(resolve_force_category(&lm_client, fc, &mut lm_category_names).await?),
            _ => None,
        };

        let sw_category_id_to_path = fetch_splitwise_categories(&sw_client, &self.config).await?;

        let loan_tag_name = if opts.no_loan_tag {
            None
        } else {
            self.config.sync.loan_tag.as_deref()
        };

        // Planning step for tags (dry-run safe)
        let PlannedTags {
            tag_id,
            loan_tag_id,
            tags_to_create,
        } = plan_tags(&lm_client, opts.tag.as_deref(), loan_tag_name).await?;

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
            lm_client: &lm_client,
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
            config: &self.config,
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
                lm_client: &lm_client,
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

pub(crate) async fn run_sync_window(sync_args: crate::cli::SyncWindowArgs) -> anyhow::Result<()> {
    let config = crate::load_config()?;
    let orchestrator = SyncOrchestrator::new(config);
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

pub(crate) async fn run_sync_group(sync_args: crate::cli::SyncGroupArgs) -> anyhow::Result<()> {
    let config = crate::load_config()?;

    // Resolve group name for default CSV filename if needed
    let http_pool = reqwest::Client::new();
    let sw_client = crate::api::splitwise::Client::new(http_pool, config.splitwise.api_key.clone());
    let groups = sw_client.fetch_groups().await?;
    let target_group = super::resolve_group(&groups, &sync_args.group)?;

    let csv_path = match sync_args.csv {
        Some(Some(path)) => Some(path),
        Some(None) => {
            let filename = format!("{}.csv", target_group.name);
            Some(std::path::PathBuf::from(filename))
        }
        None => None,
    };

    let orchestrator = SyncOrchestrator::new(config);
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

struct DiffTransactionsArgs<'a> {
    expenses: Vec<crate::api::splitwise::schema::Expense>,
    config: &'a crate::config::Config,
    target_accounts: &'a HashMap<crate::api::Currency, u64>,
    group_map: &'a HashMap<u64, String>,
    lm_map: &'a mut HashMap<crate::api::ExternalId, Transaction>,
    sw_category_id_to_path: &'a HashMap<u32, String>,
    resolved_categories: &'a HashMap<String, u64>,
    ignored_groups_exclude: Option<u64>,
    bypass_ignore_groups: bool,
    tag_id: Option<u64>,
    loan_tag_id: Option<u64>,
    force_category_id: Option<u64>,
    tags_to_create: Vec<String>,
}

fn diff_transactions(args: DiffTransactionsArgs<'_>) -> anyhow::Result<SyncPlan> {
    let DiffTransactionsArgs {
        expenses,
        config,
        target_accounts,
        group_map,
        lm_map,
        sw_category_id_to_path,
        resolved_categories,
        ignored_groups_exclude,
        bypass_ignore_groups,
        tag_id,
        loan_tag_id,
        force_category_id,
        tags_to_create,
    } = args;
    let mut inserts = Vec::new();
    let mut updates = Vec::new();
    let mut deletes = Vec::new();

    for expense in expenses {
        let external_id = crate::api::ExternalId::Splitwise(expense.id);

        let net_balance = expense
            .users
            .iter()
            .find(|u| u.user_id == config.splitwise.user_id)
            .map(|u| u.net_balance)
            .unwrap_or(Decimal::ZERO);

        let is_ignored = !bypass_ignore_groups
            && expense.group_id.is_some_and(|gid| {
                let name = group_map.get(&gid).map(|s| s.as_str());
                config.splitwise.is_group_ignored(gid, name) && Some(gid) != ignored_groups_exclude
            });

        // Skip ignored, deleted, or un-involved expenses
        if expense.deleted_at.is_some() || is_ignored || net_balance.is_zero() {
            if let Some(existing_lm) = lm_map.remove(&external_id) {
                if existing_lm.is_split_parent != Some(true) {
                    deletes.push(existing_lm);
                }
            }
            continue;
        }

        if !target_accounts.contains_key(&expense.currency_code) {
            anyhow::bail!(
                "No manual account configured for currency '{}'.\n\
                Please set up an active 'Splitwise {}' manual account in Lunch Money or configure [lunch_money.custom_accounts].",
                expense.currency_code,
                expense.currency_code
            );
        }

        let date_civil = expense.date.to_zoned(jiff::tz::TimeZone::UTC).date();

        let payee_str = if expense.group_id.is_none() {
            super::resolve_splitwise_payee(&expense, config.splitwise.user_id, group_map)
        } else {
            format!(
                "Splitwise - {}",
                super::resolve_splitwise_payee(&expense, config.splitwise.user_id, group_map)
            )
        };

        if let Some(existing_lm) = lm_map.remove(&external_id) {
            if existing_lm.is_split_parent == Some(true) {
                continue;
            }
            let amount_changed = existing_lm.amount != net_balance;

            if amount_changed || existing_lm.currency != expense.currency_code {
                updates.push(UpdateObject {
                    id: existing_lm.id,
                    date: existing_lm.date,
                    amount: net_balance,
                    currency: expense.currency_code.clone(),
                    payee: existing_lm.payee.clone(),
                    notes: existing_lm.notes.clone().unwrap_or_default(),
                });
            }
        } else {
            let manual_account_id = target_accounts[&expense.currency_code];
            let mut category_id = None;
            if force_category_id.is_some() {
                category_id = force_category_id;
            } else if expense.payment {
                category_id = resolved_categories.get("Payment").copied();
            } else if let Some(ref cat) = expense.category {
                let path = sw_category_id_to_path.get(&cat.id);
                category_id = path
                    .and_then(|p| resolved_categories.get(p))
                    .or_else(|| resolved_categories.get(&cat.name))
                    .or_else(|| resolved_categories.get(&cat.id.to_string()))
                    .copied();
            }

            let mut tx_tag_ids = Vec::new();
            if let Some(tid) = tag_id {
                tx_tag_ids.push(tid);
            }
            if net_balance > Decimal::ZERO {
                if let Some(ltid) = loan_tag_id {
                    tx_tag_ids.push(ltid);
                }
            }
            let tag_ids_opt = if tx_tag_ids.is_empty() {
                None
            } else {
                Some(tx_tag_ids)
            };

            inserts.push(InsertObject {
                date: date_civil,
                amount: net_balance,
                currency: expense.currency_code.clone(),
                payee: payee_str,
                notes: expense.description,
                external_id,
                manual_account_id,
                status: crate::api::lunch_money::schema::TransactionStatus::Unreviewed,
                tag_ids: tag_ids_opt,
                category_id,
            });
        }
    }

    Ok(SyncPlan {
        inserts,
        updates,
        deletes,
        tags_to_create,
    })
}

struct PrintAndLogSyncPlanArgs<'a> {
    plan: &'a SyncPlan,
    dry_run: bool,
    lm_category_names: &'a HashMap<u64, String>,
    sw_expense_categories: &'a HashMap<crate::api::ExternalId, Option<(u32, String)>>,
    sw_category_id_to_path: &'a HashMap<u32, String>,
    lm_tx_categories: &'a HashMap<u64, (Option<crate::api::ExternalId>, Option<u64>)>,
    csv_path: Option<&'a std::path::Path>,
}

fn print_and_log_sync_plan(args: PrintAndLogSyncPlanArgs<'_>) -> anyhow::Result<()> {
    let PrintAndLogSyncPlanArgs {
        plan,
        dry_run,
        lm_category_names,
        sw_expense_categories,
        sw_category_id_to_path,
        lm_tx_categories,
        csv_path,
    } = args;

    if let Some(path) = csv_path {
        #[derive(serde::Serialize)]
        struct CsvRow<'a> {
            operation: &'static str,
            lunch_money_id: Option<u64>,
            external_id: Option<String>,
            date: String,
            payee: &'a str,
            amount: Decimal,
            currency: &'a str,
            notes: &'a str,
            category: &'a str,
        }

        let mut wtr = csv::Writer::from_path(path)
            .with_context(|| format!("Failed to create CSV file at '{}'", path.display()))?;

        // Write deletes
        for t in &plan.deletes {
            let category_name = t
                .category_id
                .and_then(|id| lm_category_names.get(&id).cloned())
                .unwrap_or_default();
            let ext_id_str = t.external_id.as_ref().map(|ext_id| ext_id.to_string());
            wtr.serialize(CsvRow {
                operation: "delete",
                lunch_money_id: Some(t.id),
                external_id: ext_id_str,
                date: t.date.to_string(),
                payee: &t.payee,
                amount: t.amount,
                currency: t.currency.as_str(),
                notes: t.notes.as_deref().unwrap_or(""),
                category: &category_name,
            })
            .context("Failed to write CSV row")?;
        }

        // Write updates
        for u in &plan.updates {
            let (external_id, category_id) = lm_tx_categories
                .get(&u.id)
                .map(|(ext_id, cat_id)| (ext_id.as_ref(), *cat_id))
                .unwrap_or((None, None));
            let category_name = category_id
                .and_then(|id| lm_category_names.get(&id).cloned())
                .unwrap_or_default();
            let ext_id_str = external_id.map(|ext_id| ext_id.to_string());
            wtr.serialize(CsvRow {
                operation: "update",
                lunch_money_id: Some(u.id),
                external_id: ext_id_str,
                date: u.date.to_string(),
                payee: &u.payee,
                amount: u.amount,
                currency: u.currency.as_str(),
                notes: &u.notes,
                category: &category_name,
            })
            .context("Failed to write CSV row")?;
        }

        // Write inserts
        for ins in &plan.inserts {
            let category_name = ins
                .category_id
                .and_then(|id| lm_category_names.get(&id).cloned())
                .unwrap_or_default();
            wtr.serialize(CsvRow {
                operation: "insert",
                lunch_money_id: None,
                external_id: Some(ins.external_id.to_string()),
                date: ins.date.to_string(),
                payee: &ins.payee,
                amount: ins.amount,
                currency: ins.currency.as_str(),
                notes: &ins.notes,
                category: &category_name,
            })
            .context("Failed to write CSV row")?;
        }

        wtr.flush().context("Failed to flush CSV file")?;
    }

    if dry_run {
        for tag_name in &plan.tags_to_create {
            println! { "   {STYLE_WARNING}Would create tag:{STYLE_WARNING:#} '{}'", tag_name };
        }
    }

    // Execute batches output
    if !plan.deletes.is_empty() {
        println! { "🗑️  {STYLE_WARNING}Deleting {STYLE_WARNING:#}{} old/modified transaction(s) from Lunch Money:", plan.deletes.len() };
        let super::MaxWidths {
            max_num_len,
            max_currency_len,
        } = super::compute_max_widths(plan.deletes.iter().map(|t| (t.amount, &t.currency)));
        let mut records = Vec::new();
        for t in &plan.deletes {
            let category_name = t
                .category_id
                .and_then(|id| lm_category_names.get(&id).cloned());
            let sw_category_name = t
                .external_id
                .as_ref()
                .and_then(|ext_id| sw_expense_categories.get(ext_id))
                .and_then(|cat_info| {
                    cat_info.as_ref().and_then(|(cat_id, cat_name)| {
                        sw_category_id_to_path
                            .get(cat_id)
                            .map(|s| s.as_str())
                            .or(Some(cat_name.as_str()))
                    })
                });
            records.push(to_sync_record(ToSyncRecordArgs {
                payee: &t.payee,
                amount: t.amount,
                currency: &t.currency,
                date: t.date,
                notes: t.notes.as_deref().unwrap_or(""),
                sw_category_name,
                lm_category_name: category_name.as_deref(),
                max_num_len,
                max_currency_len,
            }));
        }
        let mut table = Table::new(records);
        table.with(Style::rounded());
        println! { "{}" , table };
        println! {};
    }

    if !plan.updates.is_empty() {
        println! { "✎  {STYLE_INFO}Updating {STYLE_INFO:#}{} modified transaction(s) in Lunch Money:", plan.updates.len() };
        let super::MaxWidths {
            max_num_len,
            max_currency_len,
        } = super::compute_max_widths(plan.updates.iter().map(|u| (u.amount, &u.currency)));
        let mut records = Vec::new();
        for u in &plan.updates {
            let (external_id, category_id) = lm_tx_categories
                .get(&u.id)
                .map(|(ext_id, cat_id)| (ext_id.as_ref(), *cat_id))
                .unwrap_or((None, None));
            let sw_category_name = external_id
                .and_then(|ext_id| sw_expense_categories.get(ext_id))
                .and_then(|cat_info| {
                    cat_info.as_ref().and_then(|(cat_id, cat_name)| {
                        sw_category_id_to_path
                            .get(cat_id)
                            .map(|s| s.as_str())
                            .or(Some(cat_name.as_str()))
                    })
                });
            let category_name = category_id.and_then(|id| lm_category_names.get(&id).cloned());
            records.push(to_sync_record(ToSyncRecordArgs {
                payee: &u.payee,
                amount: u.amount,
                currency: &u.currency,
                date: u.date,
                notes: &u.notes,
                sw_category_name,
                lm_category_name: category_name.as_deref(),
                max_num_len,
                max_currency_len,
            }));
        }
        let mut table = Table::new(records);
        table.with(Style::rounded());
        println! { "{}" , table };
        println! {};
    }

    if !plan.inserts.is_empty() {
        println! { "✓  {STYLE_SUCCESS}Inserting {STYLE_SUCCESS:#}{} new transaction(s) to Lunch Money:", plan.inserts.len() };
        let super::MaxWidths {
            max_num_len,
            max_currency_len,
        } = super::compute_max_widths(plan.inserts.iter().map(|ins| (ins.amount, &ins.currency)));
        let mut records = Vec::new();
        for ins in &plan.inserts {
            let category_name = ins
                .category_id
                .and_then(|id| lm_category_names.get(&id).cloned());
            let sw_category_name =
                sw_expense_categories
                    .get(&ins.external_id)
                    .and_then(|cat_info| {
                        cat_info.as_ref().and_then(|(cat_id, cat_name)| {
                            sw_category_id_to_path
                                .get(cat_id)
                                .map(|s| s.as_str())
                                .or(Some(cat_name.as_str()))
                        })
                    });
            records.push(to_sync_record(ToSyncRecordArgs {
                payee: &ins.payee,
                amount: ins.amount,
                currency: &ins.currency,
                date: ins.date,
                notes: &ins.notes,
                sw_category_name,
                lm_category_name: category_name.as_deref(),
                max_num_len,
                max_currency_len,
            }));
        }
        let mut table = Table::new(records);
        table.with(Style::rounded());
        println! { "{}" , table };
        println! {};
    }

    if plan.deletes.is_empty() && plan.updates.is_empty() && plan.inserts.is_empty() {
        println! { "{STYLE_SUCCESS}✨ No changes detected. Lunch Money manual account is up-to-date!{STYLE_SUCCESS:#}" };
    } else if dry_run {
        println! { "{STYLE_WARNING}⚠️ Dry run complete! No changes were made to Lunch Money.{STYLE_WARNING:#}" };
    }
    println! {};
    Ok(())
}

struct ApplySyncPlanArgs<'a> {
    plan: &'a mut SyncPlan,
    lm_client: &'a crate::api::lunch_money::Client,
    manual_accounts: &'a [crate::api::lunch_money::schema::ManualAccount],
    target_accounts: &'a HashMap<crate::api::Currency, u64>,
    tag_name: Option<&'a str>,
    loan_tag_name: Option<&'a str>,
}

async fn apply_sync_plan(args: ApplySyncPlanArgs<'_>) -> anyhow::Result<()> {
    let ApplySyncPlanArgs {
        plan,
        lm_client,
        manual_accounts,
        target_accounts,
        tag_name,
        loan_tag_name,
    } = args;

    let mut tag_id_map = HashMap::new();
    for name in &plan.tags_to_create {
        println! { "  {STYLE_DIM}Creating new tag '{}'...{STYLE_DIM:#}", name };
        let new_tag = lm_client.create_tag(name).await?;
        tag_id_map.insert(name.clone(), new_tag.id);
    }

    let created_tag_id = tag_name.and_then(|name| tag_id_map.get(name).copied());
    let created_loan_tag_id = loan_tag_name.and_then(|name| tag_id_map.get(name).copied());

    if created_tag_id.is_some() || created_loan_tag_id.is_some() {
        for ins in &mut plan.inserts {
            let mut ids = ins.tag_ids.take().unwrap_or_default();
            if let Some(id) = created_tag_id {
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
            if ins.amount > Decimal::ZERO {
                if let Some(id) = created_loan_tag_id {
                    if !ids.contains(&id) {
                        ids.push(id);
                    }
                }
            }
            if !ids.is_empty() {
                ins.tag_ids = Some(ids);
            }
        }
    }

    if !plan.deletes.is_empty() {
        let delete_ids: Vec<u64> = plan.deletes.iter().map(|t| t.id).collect();
        lm_client.delete_transactions(&delete_ids).await?;
    }

    if !plan.updates.is_empty() {
        for chunk in plan.updates.chunks(500) {
            let mut chunk_txs = chunk.to_vec();
            for u in &mut chunk_txs {
                let is_loan = manual_accounts
                    .iter()
                    .find(|acc| target_accounts.get(&u.currency).copied() == Some(acc.id))
                    .map(|acc| {
                        acc.account_type == crate::api::lunch_money::schema::AccountType::Loan
                    })
                    .unwrap_or(false);
                if is_loan {
                    u.amount = -u.amount;
                }
            }
            lm_client.update_transactions(&chunk_txs).await?;
        }
    }

    if !plan.inserts.is_empty() {
        for chunk in plan.inserts.chunks(500) {
            let mut chunk_txs = chunk.to_vec();
            for ins in &mut chunk_txs {
                let is_loan = manual_accounts
                    .iter()
                    .find(|acc| acc.id == ins.manual_account_id)
                    .map(|acc| {
                        acc.account_type == crate::api::lunch_money::schema::AccountType::Loan
                    })
                    .unwrap_or(false);
                if is_loan {
                    ins.amount = -ins.amount;
                }
            }
            lm_client.insert_transactions(&chunk_txs).await?;
        }
    }

    if !plan.deletes.is_empty() || !plan.updates.is_empty() || !plan.inserts.is_empty() {
        println! { "{STYLE_SUCCESS}✨ Synchronization cycle complete!{STYLE_SUCCESS:#}" };
        println! {};
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diff_transactions_loan_tag() {
        let config_str = r#"
            [splitwise]
            api_key = "dummy"
            user_id = 123
            ignored_groups = []

            [lunch_money]
            api_key = "dummy"
            custom_accounts = { USD = 999 }
        "#;
        let config: crate::config::Config = toml::from_str(config_str).unwrap();

        let expenses_json = r#"[
            {
                "id": 1,
                "description": "Positive Net Balance (folks owe me)",
                "date": "2026-06-06T12:00:00Z",
                "currency_code": "USD",
                "users": [
                    {
                        "user_id": 123,
                        "net_balance": "50.00"
                    }
                ],
                "payment": false
            },
            {
                "id": 2,
                "description": "Negative Net Balance (I owe folks)",
                "date": "2026-06-06T12:00:00Z",
                "currency_code": "USD",
                "users": [
                    {
                        "user_id": 123,
                        "net_balance": "-20.00"
                    }
                ],
                "payment": false
            }
        ]"#;
        let expenses: Vec<crate::api::splitwise::schema::Expense> =
            serde_json::from_str(expenses_json).unwrap();

        let mut target_accounts = HashMap::new();
        target_accounts.insert(crate::api::Currency::new("USD"), 999);

        let mut lm_map = HashMap::new();
        let sw_category_id_to_path = HashMap::new();
        let resolved_categories = HashMap::new();

        let plan = diff_transactions(DiffTransactionsArgs {
            expenses,
            config: &config,
            target_accounts: &target_accounts,
            group_map: &HashMap::new(),
            lm_map: &mut lm_map,
            sw_category_id_to_path: &sw_category_id_to_path,
            resolved_categories: &resolved_categories,
            ignored_groups_exclude: None,
            bypass_ignore_groups: false,
            tag_id: Some(444),
            loan_tag_id: Some(555),
            force_category_id: None,
            tags_to_create: vec![],
        })
        .unwrap();

        let inserts = plan.inserts;
        let updates = plan.updates;
        let deletes = plan.deletes;

        assert!(updates.is_empty());
        assert!(deletes.is_empty());
        assert_eq!(inserts.len(), 2);

        // Transaction 1: net_balance is 50.00 (positive). Should have both tags.
        let tx1 = inserts
            .iter()
            .find(|tx| tx.amount == Decimal::new(5000, 2))
            .unwrap();
        assert_eq!(tx1.tag_ids, Some(vec![444, 555]));

        // Transaction 2: net_balance is -20.00 (negative). Should only have tag_id.
        let tx2 = inserts
            .iter()
            .find(|tx| tx.amount == Decimal::new(-2000, 2))
            .unwrap();
        assert_eq!(tx2.tag_ids, Some(vec![444]));
    }

    #[test]
    fn test_diff_transactions_no_loan_tag() {
        let config_str = r#"
            [splitwise]
            api_key = "dummy"
            user_id = 123
            ignored_groups = []

            [lunch_money]
            api_key = "dummy"
            custom_accounts = { USD = 999 }
        "#;
        let config: crate::config::Config = toml::from_str(config_str).unwrap();

        let expenses_json = r#"[
            {
                "id": 1,
                "description": "Positive Net Balance (folks owe me)",
                "date": "2026-06-06T12:00:00Z",
                "currency_code": "USD",
                "users": [
                    {
                        "user_id": 123,
                        "net_balance": "50.00"
                    }
                ],
                "payment": false
            }
        ]"#;
        let expenses: Vec<crate::api::splitwise::schema::Expense> =
            serde_json::from_str(expenses_json).unwrap();

        let mut target_accounts = HashMap::new();
        target_accounts.insert(crate::api::Currency::new("USD"), 999);

        let mut lm_map = HashMap::new();
        let sw_category_id_to_path = HashMap::new();
        let resolved_categories = HashMap::new();

        // Pass None for loan_tag_id
        let plan = diff_transactions(DiffTransactionsArgs {
            expenses,
            config: &config,
            target_accounts: &target_accounts,
            group_map: &HashMap::new(),
            lm_map: &mut lm_map,
            sw_category_id_to_path: &sw_category_id_to_path,
            resolved_categories: &resolved_categories,
            ignored_groups_exclude: None,
            bypass_ignore_groups: false,
            tag_id: Some(444),
            loan_tag_id: None,
            force_category_id: None,
            tags_to_create: vec![],
        })
        .unwrap();

        let inserts = plan.inserts;
        let updates = plan.updates;
        let deletes = plan.deletes;

        assert!(updates.is_empty());
        assert!(deletes.is_empty());
        assert_eq!(inserts.len(), 1);

        // Transaction 1: net_balance is 50.00 (positive). Should only have tag_id, not loan_tag_id.
        let tx1 = inserts
            .iter()
            .find(|tx| tx.amount == Decimal::new(5000, 2))
            .unwrap();
        assert_eq!(tx1.tag_ids, Some(vec![444]));
    }

    #[tokio::test]
    async fn test_execute_sync_actions_csv() {
        use crate::api::Currency;
        use crate::api::ExternalId;
        use crate::api::lunch_money::schema::InsertObject;
        use crate::api::lunch_money::schema::Transaction;
        use crate::api::lunch_money::schema::TransactionStatus;
        use crate::api::lunch_money::schema::UpdateObject;
        use rust_decimal::Decimal;
        use std::collections::HashMap;

        let temp_dir = std::env::temp_dir();
        let csv_path = temp_dir.join("sync_actions_test.csv");
        if csv_path.exists() {
            let _ = std::fs::remove_file(&csv_path);
        }

        let deletes = vec![Transaction {
            id: 10,
            date: jiff::civil::date(2026, 6, 1),
            amount: Decimal::new(-1000, 2),
            currency: Currency::new("USD"),
            payee: "Delete Payee".to_string(),
            notes: Some("Delete Notes".to_string()),
            external_id: Some(ExternalId::Splitwise(100)),
            manual_account_id: Some(999),
            is_split_parent: None,
            group_parent_id: None,
            status: TransactionStatus::Reviewed,
            category_id: Some(5),
        }];

        let updates = vec![UpdateObject {
            id: 20,
            date: jiff::civil::date(2026, 6, 2),
            amount: Decimal::new(2000, 2),
            currency: Currency::new("USD"),
            payee: "Update Payee".to_string(),
            notes: "Update Notes".to_string(),
        }];

        let inserts = vec![InsertObject {
            date: jiff::civil::date(2026, 6, 3),
            amount: Decimal::new(3000, 2),
            currency: Currency::new("USD"),
            payee: "Insert Payee".to_string(),
            notes: "Insert Notes".to_string(),
            external_id: ExternalId::Splitwise(300),
            manual_account_id: 999,
            status: TransactionStatus::Unreviewed,
            tag_ids: None,
            category_id: Some(6),
        }];

        let mut lm_category_names = HashMap::new();
        lm_category_names.insert(5, "Delete Category".to_string());
        lm_category_names.insert(6, "Insert Category".to_string());
        lm_category_names.insert(7, "Update Category".to_string());

        let mut sw_expense_categories = HashMap::new();
        sw_expense_categories.insert(
            ExternalId::Splitwise(100),
            Some((100, "SW Cat".to_string())),
        );

        let sw_category_id_to_path = HashMap::new();

        let mut lm_tx_categories = HashMap::new();
        lm_tx_categories.insert(20, (Some(ExternalId::Splitwise(200)), Some(7)));

        let plan = SyncPlan {
            inserts,
            updates,
            deletes,
            tags_to_create: vec![],
        };

        print_and_log_sync_plan(PrintAndLogSyncPlanArgs {
            plan: &plan,
            dry_run: true,
            lm_category_names: &lm_category_names,
            sw_expense_categories: &sw_expense_categories,
            sw_category_id_to_path: &sw_category_id_to_path,
            lm_tx_categories: &lm_tx_categories,
            csv_path: Some(&csv_path),
        })
        .unwrap();

        assert!(csv_path.exists());
        let content = std::fs::read_to_string(&csv_path).unwrap();
        let lines: Vec<&str> = content.lines().collect();

        assert_eq!(lines.len(), 4);
        assert_eq!(
            lines[0],
            "operation,lunch_money_id,external_id,date,payee,amount,currency,notes,category"
        );
        assert_eq!(
            lines[1],
            "delete,10,splitwise_100,2026-06-01,Delete Payee,-10.00,USD,Delete Notes,Delete Category"
        );
        assert_eq!(
            lines[2],
            "update,20,splitwise_200,2026-06-02,Update Payee,20.00,USD,Update Notes,Update Category"
        );
        assert_eq!(
            lines[3],
            "insert,,splitwise_300,2026-06-03,Insert Payee,30.00,USD,Insert Notes,Insert Category"
        );

        let _ = std::fs::remove_file(csv_path);
    }

    #[test]
    fn test_diff_transactions_force_category() {
        let config_str = r#"
            [splitwise]
            api_key = "dummy"
            user_id = 123
            ignored_groups = []

            [lunch_money]
            api_key = "dummy"
            custom_accounts = { USD = 999 }
        "#;
        let config: crate::config::Config = toml::from_str(config_str).unwrap();

        let expenses_json = r#"[
            {
                "id": 1,
                "description": "Forced category expense",
                "date": "2026-06-06T12:00:00Z",
                "currency_code": "USD",
                "users": [
                    {
                        "user_id": 123,
                        "net_balance": "50.00"
                    }
                ],
                "payment": false
            }
        ]"#;
        let expenses: Vec<crate::api::splitwise::schema::Expense> =
            serde_json::from_str(expenses_json).unwrap();

        let mut target_accounts = HashMap::new();
        target_accounts.insert(crate::api::Currency::new("USD"), 999);

        let mut lm_map = HashMap::new();
        let sw_category_id_to_path = HashMap::new();
        let resolved_categories = HashMap::new();

        let plan = diff_transactions(DiffTransactionsArgs {
            expenses,
            config: &config,
            target_accounts: &target_accounts,
            group_map: &HashMap::new(),
            lm_map: &mut lm_map,
            sw_category_id_to_path: &sw_category_id_to_path,
            resolved_categories: &resolved_categories,
            ignored_groups_exclude: None,
            bypass_ignore_groups: false,
            tag_id: None,
            loan_tag_id: None,
            force_category_id: Some(777),
            tags_to_create: vec![],
        })
        .unwrap();

        let inserts = plan.inserts;
        let updates = plan.updates;
        let deletes = plan.deletes;

        assert!(updates.is_empty());
        assert!(deletes.is_empty());
        assert_eq!(inserts.len(), 1);
        assert_eq!(inserts[0].category_id, Some(777));
    }

    #[test]
    fn test_individual_payee_formatting() {
        let config_str = r#"
            [splitwise]
            api_key = "dummy"
            user_id = 123
            ignored_groups = []

            [lunch_money]
            api_key = "dummy"
            custom_accounts = { USD = 999 }
        "#;
        let config: crate::config::Config = toml::from_str(config_str).unwrap();

        // 1. Non-group expense (individual)
        // One other user in the expense should provide the name
        let expenses_json = r#"[
            {
                "id": 1,
                "description": "Lunch with Alice",
                "date": "2026-06-06T12:00:00Z",
                "currency_code": "USD",
                "users": [
                    {
                        "user_id": 123,
                        "net_balance": "50.00"
                    },
                    {
                        "user_id": 456,
                        "net_balance": "-50.00",
                        "user": {
                            "first_name": "Alice",
                            "last_name": "Smith"
                        }
                    }
                ],
                "payment": false
            },
            {
                "id": 2,
                "group_id": 789,
                "description": "Group dinner",
                "date": "2026-06-06T12:00:00Z",
                "currency_code": "USD",
                "users": [
                    {
                        "user_id": 123,
                        "net_balance": "-20.00"
                    }
                ],
                "payment": false
            }
        ]"#;
        let expenses: Vec<crate::api::splitwise::schema::Expense> =
            serde_json::from_str(expenses_json).unwrap();

        let mut target_accounts = HashMap::new();
        target_accounts.insert(crate::api::Currency::new("USD"), 999);

        let mut lm_map = HashMap::new();
        let sw_category_id_to_path = HashMap::new();
        let resolved_categories = HashMap::new();

        let mut group_map = HashMap::new();
        group_map.insert(789, "Roommates".to_string());

        let plan = diff_transactions(DiffTransactionsArgs {
            expenses,
            config: &config,
            target_accounts: &target_accounts,
            group_map: &group_map,
            lm_map: &mut lm_map,
            sw_category_id_to_path: &sw_category_id_to_path,
            resolved_categories: &resolved_categories,
            ignored_groups_exclude: None,
            bypass_ignore_groups: false,
            tag_id: None,
            loan_tag_id: None,
            force_category_id: None,
            tags_to_create: vec![],
        })
        .unwrap();

        let inserts = plan.inserts;

        assert_eq!(inserts.len(), 2);

        // Individual expense (id: 1) should have payee "Alice Smith" (no "Splitwise - " prefix)
        let tx_individual = inserts
            .iter()
            .find(|tx| tx.external_id == crate::api::ExternalId::Splitwise(1))
            .unwrap();
        assert_eq!(tx_individual.payee, "Alice Smith");

        // Group expense (id: 2) should have payee "Splitwise - Roommates"
        let tx_group = inserts
            .iter()
            .find(|tx| tx.external_id == crate::api::ExternalId::Splitwise(2))
            .unwrap();
        assert_eq!(tx_group.payee, "Splitwise - Roommates");
    }

    #[test]
    fn test_diff_transactions_no_ignore() {
        let config_str = r#"
            [splitwise]
            api_key = "dummy"
            user_id = 123
            ignored_groups = [ 789 ]

            [lunch_money]
            api_key = "dummy"
            custom_accounts = { USD = 999 }
        "#;
        let config: crate::config::Config = toml::from_str(config_str).unwrap();

        let expenses_json = r#"[
            {
                "id": 1,
                "description": "Group expense",
                "date": "2026-06-06T12:00:00Z",
                "currency_code": "USD",
                "group_id": 789,
                "users": [
                    {
                        "user_id": 123,
                        "net_balance": "50.00"
                    }
                ],
                "payment": false
            }
        ]"#;
        let expenses1: Vec<crate::api::splitwise::schema::Expense> =
            serde_json::from_str(expenses_json).unwrap();
        let expenses2: Vec<crate::api::splitwise::schema::Expense> =
            serde_json::from_str(expenses_json).unwrap();

        let mut target_accounts = HashMap::new();
        target_accounts.insert(crate::api::Currency::new("USD"), 999);

        let mut lm_map = HashMap::new();
        let sw_category_id_to_path = HashMap::new();
        let resolved_categories = HashMap::new();

        let mut group_map = HashMap::new();
        group_map.insert(789, "Roommates".to_string());

        // Case 1: bypass_ignore_groups = false (should be ignored, inserts is empty)
        let plan1 = diff_transactions(DiffTransactionsArgs {
            expenses: expenses1,
            config: &config,
            target_accounts: &target_accounts,
            group_map: &group_map,
            lm_map: &mut lm_map,
            sw_category_id_to_path: &sw_category_id_to_path,
            resolved_categories: &resolved_categories,
            ignored_groups_exclude: None,
            bypass_ignore_groups: false,
            tag_id: None,
            loan_tag_id: None,
            force_category_id: None,
            tags_to_create: vec![],
        })
        .unwrap();
        let inserts1 = plan1.inserts;
        assert!(inserts1.is_empty());

        // Case 2: bypass_ignore_groups = true (should NOT be ignored, inserts has 1 item)
        let plan2 = diff_transactions(DiffTransactionsArgs {
            expenses: expenses2,
            config: &config,
            target_accounts: &target_accounts,
            group_map: &group_map,
            lm_map: &mut lm_map,
            sw_category_id_to_path: &sw_category_id_to_path,
            resolved_categories: &resolved_categories,
            ignored_groups_exclude: None,
            bypass_ignore_groups: true,
            tag_id: None,
            loan_tag_id: None,
            force_category_id: None,
            tags_to_create: vec![],
        })
        .unwrap();
        let inserts2 = plan2.inserts;
        assert_eq!(inserts2.len(), 1);
    }
}
