mod diff;
mod execute;
mod report;

use crate::api::ExpensesQuery;
use crate::api::ExternalId;
use crate::api::TransactionQuery;
use crate::api::lunch_money::schema::AccountType;
use crate::api::lunch_money::schema::InsertObject;
use crate::api::lunch_money::schema::ManualAccount;
use crate::api::lunch_money::schema::Transaction;
use crate::api::lunch_money::schema::TransactionStatus;
use crate::api::lunch_money::schema::UpdateObject;
use crate::metadata::LunchMoneyTxMetadata;
use crate::metadata::MaybeLunchMoneyTxMetadata;
use crate::style::*;
use anstream::println;
use anyhow::Context;
use rust_decimal::Decimal;
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

                let txs = sw_client
                    .fetch_expenses(&ExpensesQuery {
                        dated_after: Some(start_window_str.clone()),
                        dated_before: from.map(|f| format!("{}T23:59:59Z", f)),
                        limit: Some(0),
                        ..Default::default()
                    })
                    .await?;

                let updated_after_str = format!("{}T00:00:00Z", start_window_str);
                let query2_txs = sw_client
                    .fetch_expenses(&ExpensesQuery {
                        updated_after: Some(updated_after_str),
                        dated_before: from.map(|f| format!("{}T23:59:59Z", f)),
                        limit: Some(0),
                        ..Default::default()
                    })
                    .await?;

                let mut tx_map = HashMap::new();
                for e in txs {
                    tx_map.insert(e.parsed.id, e);
                }
                for e in query2_txs {
                    tx_map.insert(e.parsed.id, e);
                }

                let mut merged_txs: Vec<_> = tx_map.into_values().collect();
                if *no_groups {
                    merged_txs.retain(|e| e.parsed.group_id.is_none());
                }
                merged_txs
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
            let ext_id = ExternalId::Splitwise(expense.parsed.id);
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

        let backdated_tag_name = Some(self.ctx.config.sync.backdated_tag.as_str());
        let updated_tag_name = Some(self.ctx.config.sync.updated_tag.as_str());
        let orphaned_tag_name = Some(self.ctx.config.sync.orphaned_tag.as_str());

        // Planning step for tags (dry-run safe)
        let PlannedTags {
            tag_id,
            loan_tag_id,
            backdated_tag_id,
            updated_tag_id,
            orphaned_tag_id,
            tags_to_create,
        } = plan_tags(
            lm_client,
            opts.dry_run,
            opts.tag.as_deref(),
            loan_tag_name,
            backdated_tag_name,
            updated_tag_name,
            orphaned_tag_name,
        )
        .await?;

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

        let sync_window_start = match &opts.mode {
            SyncMode::Window { .. } => Some(start_date_str.parse::<jiff::civil::Date>()?),
            _ => None,
        };

        let lm_transactions = fetch_lunch_money_transactions(FetchLunchMoneyTransactionsArgs {
            lm_client,
            target_accounts: &target_accounts,
            manual_accounts: &manual_accounts,
            start_date_str: &start_date_str,
            end_date_str: &end_date_str,
        })
        .await?;

        // Tag-Based Pre-fetching for backdated transactions
        let mut pre_fetched_backdated = Vec::new();
        if let Some(bt_id) = backdated_tag_id {
            println! { "  {STYLE_DIM}Pre-fetching backdated transactions...{STYLE_DIM:#}" };
            for &account_id in target_accounts.values() {
                let mut txs = lm_client
                    .fetch_transactions(&TransactionQuery {
                        start_date: "2000-01-01".to_string(),
                        end_date: end_date_str.clone(),
                        manual_account_id: account_id,
                        limit: Some(1000),
                        include_group_children: Some(true),
                        include_split_parents: Some(true),
                        include_metadata: Some(true),
                        tag_id: Some(bt_id),
                    })
                    .await?;

                let is_loan = manual_accounts
                    .iter()
                    .find(|acc| acc.id == account_id)
                    .map(|acc| acc.account_type == AccountType::Loan)
                    .unwrap_or(false);

                if is_loan {
                    for t in &mut txs {
                        t.amount = -t.amount;
                    }
                }
                pre_fetched_backdated.extend(txs);
            }
        }

        // Tag-Based Pre-fetching for orphaned transactions
        let mut pre_fetched_orphaned = Vec::new();
        if let Some(ot_id) = orphaned_tag_id {
            println! { "  {STYLE_DIM}Pre-fetching orphaned transactions...{STYLE_DIM:#}" };
            for &account_id in target_accounts.values() {
                let mut txs = lm_client
                    .fetch_transactions(&TransactionQuery {
                        start_date: "2000-01-01".to_string(),
                        end_date: end_date_str.clone(),
                        manual_account_id: account_id,
                        limit: Some(1000),
                        include_group_children: Some(true),
                        include_split_parents: Some(true),
                        include_metadata: Some(true),
                        tag_id: Some(ot_id),
                    })
                    .await?;

                let is_loan = manual_accounts
                    .iter()
                    .find(|acc| acc.id == account_id)
                    .map(|acc| acc.account_type == AccountType::Loan)
                    .unwrap_or(false);

                if is_loan {
                    for t in &mut txs {
                        t.amount = -t.amount;
                    }
                }
                pre_fetched_orphaned.extend(txs);
            }
        }

        // Merge backdated and orphaned transactions, avoiding duplicates by ID
        let mut lm_tx_map_dedup = HashMap::new();
        for t in lm_transactions {
            lm_tx_map_dedup.insert(t.id, t);
        }
        for t in pre_fetched_backdated {
            lm_tx_map_dedup.insert(t.id, t);
        }
        for t in pre_fetched_orphaned {
            lm_tx_map_dedup.insert(t.id, t);
        }
        let mut lm_transactions: Vec<Transaction> = lm_tx_map_dedup.into_values().collect();

        // Identify dates of old expenses and fetch them (fallback targeted query)
        let mut old_expense_dates = Vec::new();
        if let Some(window_start) = sync_window_start {
            for e in &expenses {
                let date_civil = e.parsed.date.to_zoned(jiff::tz::TimeZone::UTC).date();
                if date_civil < window_start {
                    let ext_id = ExternalId::Splitwise(e.parsed.id);
                    if !lm_transactions
                        .iter()
                        .any(|t| t.external_id.as_ref() == Some(&ext_id))
                    {
                        old_expense_dates.push(date_civil);
                    }
                }
            }
        }

        old_expense_dates.sort();
        old_expense_dates.dedup();

        let mut date_queried_txs = Vec::new();
        for date in old_expense_dates {
            println! { "  {STYLE_DIM}Querying Lunch Money for original date {}...{STYLE_DIM:#}", date };
            let date_str = date.to_string();
            for &account_id in target_accounts.values() {
                let mut txs = lm_client
                    .fetch_transactions(&TransactionQuery {
                        start_date: date_str.clone(),
                        end_date: date_str.clone(),
                        manual_account_id: account_id,
                        limit: Some(100),
                        include_group_children: Some(true),
                        include_split_parents: Some(true),
                        include_metadata: Some(true),
                        tag_id: None,
                    })
                    .await?;

                let is_loan = manual_accounts
                    .iter()
                    .find(|acc| acc.id == account_id)
                    .map(|acc| acc.account_type == AccountType::Loan)
                    .unwrap_or(false);

                if is_loan {
                    for t in &mut txs {
                        t.amount = -t.amount;
                    }
                }
                date_queried_txs.extend(txs);
            }
        }

        if !date_queried_txs.is_empty() {
            let mut lm_tx_map_dedup = HashMap::new();
            for t in lm_transactions {
                lm_tx_map_dedup.insert(t.id, t);
            }
            for t in date_queried_txs {
                lm_tx_map_dedup.insert(t.id, t);
            }
            lm_transactions = lm_tx_map_dedup.into_values().collect();
        }

        // Pointer chasing: check if any fetched delta transaction has an original transaction
        // that hasn't been fetched yet, or vice versa.
        let mut missing_ids = std::collections::HashSet::new();
        let mut checked_ids = std::collections::HashSet::new();
        let mut deleted_ids = std::collections::HashSet::new();

        loop {
            missing_ids.clear();
            for t in &lm_transactions {
                // If it is a delta transaction, check if we fetched the original and its peer deltas
                if let Some(MaybeLunchMoneyTxMetadata::Expected(LunchMoneyTxMetadata::Delta {
                    original_transaction_id,
                    delta_transaction_ids,
                    ..
                })) = &t.custom_metadata
                {
                    if !checked_ids.contains(original_transaction_id)
                        && !lm_transactions
                            .iter()
                            .any(|tx| tx.id == *original_transaction_id)
                    {
                        missing_ids.insert(*original_transaction_id);
                    }

                    for &d_id in delta_transaction_ids {
                        if !checked_ids.contains(&d_id)
                            && !lm_transactions.iter().any(|tx| tx.id == d_id)
                        {
                            missing_ids.insert(d_id);
                        }
                    }
                }

                // If it is an import transaction, check if we fetched all its delta transactions
                if let Some(MaybeLunchMoneyTxMetadata::Expected(LunchMoneyTxMetadata::Import {
                    delta_transaction_ids,
                    ..
                })) = &t.custom_metadata
                {
                    for &d_id in delta_transaction_ids {
                        if !checked_ids.contains(&d_id)
                            && !lm_transactions.iter().any(|tx| tx.id == d_id)
                        {
                            missing_ids.insert(d_id);
                        }
                    }
                }
            }

            if missing_ids.is_empty() {
                break;
            }

            let mut fetched_txs = Vec::new();
            for &id in &missing_ids {
                println! { "  {STYLE_DIM}Pointer chasing: Fetching missing transaction ID {}...{STYLE_DIM:#}", id };
                match lm_client.fetch_transaction_by_id(id).await? {
                    Some(mut tx) => {
                        // Normalize sign if it's a Loan account
                        if let Some(acc_id) = tx.manual_account_id {
                            let is_loan = manual_accounts
                                .iter()
                                .find(|acc| acc.id == acc_id)
                                .map(|acc| acc.account_type == AccountType::Loan)
                                .unwrap_or(false);
                            if is_loan {
                                tx.amount = -tx.amount;
                            }
                        }
                        fetched_txs.push(tx);
                    }
                    None => {
                        println! { "  {STYLE_WARNING}Warning: Transaction ID {} was deleted on Lunch Money.{STYLE_WARNING:#}", id };
                        deleted_ids.insert(id);
                    }
                }
                checked_ids.insert(id);
            }

            // Merge newly fetched transactions
            let mut lm_tx_map_dedup = HashMap::new();
            for t in lm_transactions {
                lm_tx_map_dedup.insert(t.id, t);
            }
            for t in fetched_txs {
                lm_tx_map_dedup.insert(t.id, t);
            }
            lm_transactions = lm_tx_map_dedup.into_values().collect();
        }

        // --- SCENARIO A & B PROCESSING ---

        // Scenario A: A Delta transaction in an Import chain is deleted
        // Remove it from the parent transaction's delta_transaction_ids list in-memory
        for tx in &mut lm_transactions {
            if let Some(MaybeLunchMoneyTxMetadata::Expected(LunchMoneyTxMetadata::Import {
                delta_transaction_ids,
                ..
            })) = &mut tx.custom_metadata
            {
                let original_len = delta_transaction_ids.len();
                delta_transaction_ids.retain(|id| !deleted_ids.contains(id));
                if delta_transaction_ids.len() < original_len {
                    println! {
                        "  {STYLE_WARNING}Warning: Cleaned up {} deleted delta transaction reference(s) from parent transaction ID {}.{STYLE_WARNING:#}",
                        original_len - delta_transaction_ids.len(),
                        tx.id
                    };
                }
            }
        }

        // Scenario B: The Import transaction is deleted, but active Delta transactions exist
        // Group orphaned delta transactions by their original_transaction_id
        let mut orphaned_groups: HashMap<u64, Vec<Transaction>> = HashMap::new();
        for tx in &lm_transactions {
            if let Some(MaybeLunchMoneyTxMetadata::Expected(LunchMoneyTxMetadata::Delta {
                original_transaction_id,
                ..
            })) = &tx.custom_metadata
            {
                if deleted_ids.contains(original_transaction_id) {
                    orphaned_groups
                        .entry(*original_transaction_id)
                        .or_default()
                        .push(tx.clone());
                }
            }
        }

        let mut orphaned_updates = Vec::new();
        let mut orphaned_inserts = Vec::new();

        for (original_transaction_id, orphaned_deltas) in orphaned_groups {
            // Find if an Orphan transaction already exists for this original_transaction_id
            let mut existing_orphan = None;
            for tx in &lm_transactions {
                if let Some(MaybeLunchMoneyTxMetadata::Expected(LunchMoneyTxMetadata::Orphan {
                    original_transaction_id: o_id,
                    ..
                })) = &tx.custom_metadata
                {
                    if *o_id == original_transaction_id {
                        existing_orphan = Some(tx.clone());
                        break;
                    }
                }
            }

            let sum_orphaned: Decimal = orphaned_deltas.iter().map(|t| t.amount).sum();
            let target_balancing_amount = -sum_orphaned;

            // Get splitwise_id from any of the orphaned deltas
            let splitwise_id =
                if let Some(MaybeLunchMoneyTxMetadata::Expected(LunchMoneyTxMetadata::Delta {
                    splitwise_id,
                    ..
                })) = &orphaned_deltas[0].custom_metadata
                {
                    *splitwise_id
                } else {
                    0
                };

            let orphaned_transaction_ids: Vec<u64> = orphaned_deltas.iter().map(|t| t.id).collect();

            // Tag orphaned deltas if they don't have the tag (or just queue additional tags)
            if let Some(ot_id) = orphaned_tag_id {
                for tx in &orphaned_deltas {
                    orphaned_updates.push(UpdateObject {
                        id: tx.id,
                        date: tx.date,
                        amount: tx.amount,
                        currency: tx.currency.clone(),
                        payee: tx.payee.clone(),
                        notes: tx.notes.clone().unwrap_or_default(),
                        custom_metadata: tx.custom_metadata.clone().and_then(|m| match m {
                            MaybeLunchMoneyTxMetadata::Expected(meta) => Some(meta),
                            _ => None,
                        }),
                        additional_tag_ids: Some(vec![ot_id]),
                        external_id: None,
                    });
                }
            }

            let notes_str = format!(
                "Offsetting orphaned deltas for deleted transaction:{}, splitwise_id:{}",
                original_transaction_id, splitwise_id
            );

            if let Some(orphan_tx) = existing_orphan {
                // If the existing orphan amount doesn't balance out the deltas, update it
                if orphan_tx.amount != target_balancing_amount {
                    orphaned_updates.push(UpdateObject {
                        id: orphan_tx.id,
                        date: orphan_tx.date,
                        amount: target_balancing_amount,
                        currency: orphan_tx.currency.clone(),
                        payee: orphan_tx.payee.clone(),
                        notes: notes_str,
                        custom_metadata: Some(LunchMoneyTxMetadata::Orphan {
                            original_transaction_id,
                            orphaned_transaction_ids,
                            splitwise_id,
                        }),
                        additional_tag_ids: orphaned_tag_id.map(|ot_id| vec![ot_id]),
                        external_id: None,
                    });
                }
            } else {
                // Insert a new balancing transaction
                let manual_account_id = orphaned_deltas[0]
                    .manual_account_id
                    .unwrap_or_else(|| target_accounts[&orphaned_deltas[0].currency]);

                orphaned_inserts.push(InsertObject {
                    date: jiff::Timestamp::now()
                        .to_zoned(jiff::tz::TimeZone::UTC)
                        .date(),
                    amount: target_balancing_amount,
                    currency: orphaned_deltas[0].currency.clone(),
                    payee: "Splitwise - Orphaned Balance Adjustment".to_string(),
                    notes: notes_str,
                    external_id: ExternalId::Other(format!(
                        "splitwise_{}_orphan",
                        original_transaction_id
                    )),
                    manual_account_id,
                    status: TransactionStatus::Unreviewed,
                    tag_ids: orphaned_tag_id.map(|ot_id| vec![ot_id]),
                    category_id: None,
                    custom_metadata: Some(LunchMoneyTxMetadata::Orphan {
                        original_transaction_id,
                        orphaned_transaction_ids,
                        splitwise_id,
                    }),
                });
            }
        }

        // Remove orphaned deltas and existing orphan transactions from lm_transactions so they aren't compared
        lm_transactions.retain(|t| {
            if let Some(MaybeLunchMoneyTxMetadata::Expected(meta)) = &t.custom_metadata {
                match meta {
                    LunchMoneyTxMetadata::Delta {
                        original_transaction_id,
                        ..
                    } => !deleted_ids.contains(original_transaction_id),
                    LunchMoneyTxMetadata::Orphan {
                        original_transaction_id,
                        ..
                    } => !deleted_ids.contains(original_transaction_id),
                    _ => true,
                }
            } else {
                true
            }
        });

        for t in &lm_transactions {
            if let Some(ExternalId::Splitwise(_)) = &t.external_id {
                let is_valid = matches!(
                    t.custom_metadata,
                    Some(MaybeLunchMoneyTxMetadata::Expected(_))
                );
                if !is_valid {
                    anyhow::bail!(
                        "Detected previously imported Splitwise transaction (ID: {}, payee: '{}', date: {}) with missing or malformed custom_metadata.\n\
                        Please run one or more `splitwise-lunchmoney migrate` commands to repair custom_metadata for all existing transactions.",
                        t.id,
                        t.payee,
                        t.date
                    );
                }
            }
        }

        println! { "  {STYLE_DIM}Comparing transactions...{STYLE_DIM:#}" };
        println! {};

        let mut lm_tx_categories = HashMap::new();
        for t in &lm_transactions {
            lm_tx_categories.insert(t.id, (t.external_id.clone(), t.category_id));
        }

        let mut lm_map: HashMap<ExternalId, Transaction> = lm_transactions
            .iter()
            .cloned()
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
            expenses: expenses.clone(),
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
            sync_window_start,
            backdated_tag_id,
            updated_tag_id,
        })?;

        plan.updates.extend(orphaned_updates);
        plan.inserts.extend(orphaned_inserts);

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
                tag_id,
                loan_tag_id,
                updated_tag_id,
                lm_transactions: &lm_transactions,
                expenses: &expenses,
                config: &self.ctx.config,
                backdated_tag_id,
                sync_window_start,
                no_ignore: opts.no_ignore,
                lm_category_names: &lm_category_names,
                csv_path: opts.csv_path.as_deref(),
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
    manual_accounts: &[ManualAccount],
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
    manual_accounts: &'a [ManualAccount],
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
                include_metadata: Some(true),
                tag_id: None,
            })
            .await?;
        let is_loan = manual_accounts
            .iter()
            .find(|acc| acc.id == account_id)
            .map(|acc| acc.account_type == AccountType::Loan)
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
                    .filter(|c| &c.name == name && !c.archived)
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
    pub backdated_tag_id: Option<u64>,
    pub updated_tag_id: Option<u64>,
    pub orphaned_tag_id: Option<u64>,
    pub tags_to_create: Vec<String>,
}

async fn plan_tags(
    lm_client: &crate::api::lunch_money::Client,
    dry_run: bool,
    tag_name: Option<&str>,
    loan_tag_name: Option<&str>,
    backdated_tag_name: Option<&str>,
    updated_tag_name: Option<&str>,
    orphaned_tag_name: Option<&str>,
) -> anyhow::Result<PlannedTags> {
    let mut tags_to_create = Vec::new();

    if tag_name.is_none()
        && loan_tag_name.is_none()
        && backdated_tag_name.is_none()
        && updated_tag_name.is_none()
        && orphaned_tag_name.is_none()
    {
        return Ok(PlannedTags {
            tag_id: None,
            loan_tag_id: None,
            backdated_tag_id: None,
            updated_tag_id: None,
            orphaned_tag_id: None,
            tags_to_create,
        });
    }

    println! { "  {STYLE_DIM}Fetching Lunch Money tags...{STYLE_DIM:#}" };
    let mut tags = lm_client.fetch_tags().await?;

    let tag_specs = [
        (tag_name, None),
        (loan_tag_name, None),
        (
            backdated_tag_name,
            Some(
                "(splitwise-lunchmoney) Tag applied to newly inserted backdated transactions or delta adjustments posted on the current day",
            ),
        ),
        (
            updated_tag_name,
            Some(
                "(splitwise-lunchmoney) Tag applied to original/older transactions to flag that they have a newer delta adjustment",
            ),
        ),
        (
            orphaned_tag_name,
            Some(
                "(splitwise-lunchmoney) Tag applied to orphaned delta transactions when their corresponding original transaction has been deleted",
            ),
        ),
    ];
    let mut resolved_ids = [None; 5];

    for (idx, (name_opt, description)) in tag_specs.iter().copied().enumerate() {
        let name = match name_opt {
            Some(n) => n,
            None => continue,
        };

        // 1. Check if the tag already exists in Lunch Money (case-sensitive)
        if let Some(existing) = tags.iter().find(|t| t.name == name) {
            resolved_ids[idx] = Some(existing.id);
            continue;
        }

        if dry_run {
            // 2a. In dry-run, queue the tag for creation if not already queued (case-sensitive)
            if !tags_to_create.contains(&name.to_string()) {
                tags_to_create.push(name.to_string());
            }
        } else {
            // 2b. If not dry-run, create the tag immediately
            println! { "  {STYLE_DIM}Creating new tag '{}'...{STYLE_DIM:#}", name };
            let new_tag = lm_client.create_tag(name, description).await?;
            resolved_ids[idx] = Some(new_tag.id);
            // Add to local tags list so subsequent tag lookups can find/reuse it
            tags.push(new_tag);
        }
    }

    tags_to_create.sort();

    Ok(PlannedTags {
        tag_id: resolved_ids[0],
        loan_tag_id: resolved_ids[1],
        backdated_tag_id: resolved_ids[2],
        updated_tag_id: resolved_ids[3],
        orphaned_tag_id: resolved_ids[4],
        tags_to_create,
    })
}
