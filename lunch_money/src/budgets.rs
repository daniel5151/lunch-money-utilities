//! Budgets and budget summaries.

/// Query parameters for budget endpoints.
pub mod query_params {
    use serde::Serialize;

    /// Query parameters for fetching a budget summary.
    #[derive(Serialize, Debug, Clone, Default)]
    pub struct BudgetSummaryQuery {
        /// Start of date range (YYYY-MM-DD).
        pub start_date: jiff::civil::Date,
        /// End of date range (YYYY-MM-DD).
        pub end_date: jiff::civil::Date,
        /// Include categories flagged as "Exclude from Budgets".
        #[serde(skip_serializing_if = "Option::is_none")]
        pub include_exclude_from_budgets: Option<bool>,
        /// Include the occurrences array for each category.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub include_occurrences: Option<bool>,
        /// Include three budget periods prior to start_date (ignored if include_occurrences is false).
        #[serde(skip_serializing_if = "Option::is_none")]
        pub include_past_budget_dates: Option<bool>,
        /// Include a top-level totals section.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub include_totals: Option<bool>,
        /// Include a rollover pool section.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub include_rollover_pool: Option<bool>,
    }
}

/// JSON schemas for budget endpoints.
pub mod schemas {
    use crate::core::CategoryId;
    use crate::core::Currency;
    use rust_decimal::Decimal;
    use serde::Deserialize;
    use serde::Serialize;

    /// Budget period granularity setting.
    #[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
    pub enum BudgetPeriodGranularity {
        /// Daily budgeting period.
        #[serde(rename = "day")]
        Day,
        /// Weekly budgeting period.
        #[serde(rename = "week")]
        Week,
        /// Monthly budgeting period.
        #[serde(rename = "month")]
        Month,
        /// Yearly budgeting period.
        #[serde(rename = "year")]
        Year,
        /// Twice a month budgeting period.
        #[serde(rename = "twice a month")]
        TwiceAMonth,
    }

    /// Income calculation setting for budgeting.
    #[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
    pub enum BudgetIncomeOption {
        /// Maximum income.
        #[serde(rename = "max")]
        Max,
        /// Budgeted income.
        #[serde(rename = "budgeted")]
        Budgeted,
        /// Actual income activity.
        #[serde(rename = "activity")]
        Activity,
    }

    /// Settings defining the budgeting structure of the user's account.
    #[derive(Deserialize, Debug, Clone)]
    pub struct BudgetSettings {
        /// The granularity of the budgeting period.
        pub budget_period_granularity: BudgetPeriodGranularity,
        /// The number of granularity units that make up a single budgeting period.
        pub budget_period_quantity: Decimal,
        /// The date from which budgeting periods are derived.
        pub budget_period_anchor_date: jiff::civil::Date,
        /// Display preference to hide categories with no activity and no budget.
        pub budget_hide_no_activity: bool,
        /// Display preference to use the last day of the month as the end date for monthly periods.
        pub budget_use_last_day_of_month: bool,
        /// Defines which income value is used as base for calculating available funds.
        pub budget_income_option: BudgetIncomeOption,
        /// Determines if remaining unallocated funds carry forward to the next period.
        pub budget_rollover_left_to_budget: bool,
    }

    /// Response returned by the budget settings endpoint.
    #[derive(Deserialize, Debug)]
    pub struct BudgetSettingsResponse {
        /// The budget settings.
        pub budget_settings: BudgetSettings,
    }

    /// Request payload to create or update a budget.
    #[derive(Serialize, Clone, Debug)]
    pub struct UpsertBudgetRequest {
        /// Start date of the budget period (must be a valid budget period start).
        pub start_date: jiff::civil::Date,
        /// Unique identifier of the category for this budget.
        pub category_id: CategoryId,
        /// Budget amount.
        #[serde(with = "rust_decimal::serde::str")]
        pub amount: Decimal,
        /// Three-letter currency code (defaults to primary currency if omitted).
        #[serde(skip_serializing_if = "Option::is_none")]
        pub currency: Option<Currency>,
        /// Optional notes for the budget period.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub notes: Option<String>,
    }

    /// Response returned by a successful budget upsert.
    #[derive(Deserialize, Debug, Clone)]
    pub struct BudgetUpsertResponse {
        /// Category ID.
        pub category_id: CategoryId,
        /// Start date of the budget period.
        pub start_date: jiff::civil::Date,
        /// Budget amount in the stored currency.
        #[serde(with = "rust_decimal::serde::str")]
        pub amount: Decimal,
        /// Currency code of the budget.
        pub currency: Currency,
        /// Amount converted to the user's primary currency.
        pub to_base: Decimal,
        /// Notes for the budget period.
        pub notes: Option<String>,
    }

    /// Breakdown of budget summary category totals.
    #[derive(Deserialize, Debug, Clone)]
    pub struct BudgetSummaryCategoryTotals {
        /// Total non-recurring activity in user's default currency.
        pub other_activity: Decimal,
        /// Total recurring activity in user's default currency.
        pub recurring_activity: Decimal,
        /// Total budgeted amount, or `None` if not budgeted or non-aligned.
        pub budgeted: Option<Decimal>,
        /// Total funds available, or `None` if non-aligned.
        pub available: Option<Decimal>,
        /// Total expected recurring activity that has not yet occurred.
        pub recurring_remaining: Decimal,
        /// Total expected recurring activity.
        pub recurring_expected: Decimal,
    }

    /// Represents budget period activity for a category occurrence.
    #[derive(Deserialize, Debug, Clone)]
    pub struct BudgetSummaryCategoryOccurrence {
        /// True if occurrence is within requested range.
        pub in_range: bool,
        /// Start date of the budget period.
        pub start_date: jiff::civil::Date,
        /// End date of the budget period.
        pub end_date: jiff::civil::Date,
        /// Total non-recurring activity in user's default currency.
        pub other_activity: Decimal,
        /// Total recurring activity in user's default currency.
        pub recurring_activity: Decimal,
        /// Budgeted amount in primary currency.
        pub budgeted: Option<Decimal>,
        /// Budgeted amount in the budget's currency.
        pub budgeted_amount: Option<String>,
        /// Currency of the budgeted amount.
        pub budgeted_currency: Option<Currency>,
        /// Notes set for the budget period.
        pub notes: Option<String>,
    }

    /// Rollover pool information.
    #[derive(Deserialize, Debug, Clone)]
    pub struct BudgetSummaryRolloverPool {
        /// Available rollover funds in primary currency.
        pub budgeted_to_base: Decimal,
        /// List of previous adjustments.
        pub all_adjustments: Vec<BudgetSummaryRolloverPoolAdjustment>,
    }

    /// A specific adjustment to the rollover pool.
    #[derive(Deserialize, Debug, Clone)]
    pub struct BudgetSummaryRolloverPoolAdjustment {
        /// True if adjustment period falls in range.
        pub in_range: bool,
        /// Date of the adjustment.
        pub date: jiff::civil::Date,
        /// Amount of rollover pool in budget's currency.
        pub amount: String,
        /// Currency of the adjustment.
        pub currency: Currency,
        /// Amount converted to user's primary currency.
        pub to_base: Decimal,
    }

    /// Budget summary category details.
    #[derive(Deserialize, Debug, Clone)]
    pub struct BudgetSummaryCategory {
        /// Category ID.
        pub category_id: CategoryId,
        /// Category totals.
        pub totals: BudgetSummaryCategoryTotals,
        /// List of budget occurrences. Only present if include_occurrences was true.
        pub occurrences: Option<Vec<BudgetSummaryCategoryOccurrence>>,
        /// Rollover pool information. Only present if include_rollover_pool was true.
        pub rollover_pool: Option<BudgetSummaryRolloverPool>,
    }

    /// Breakdown of budget summary totals.
    #[derive(Deserialize, Debug, Clone)]
    pub struct BudgetSummaryTotalsBreakdown {
        /// Total non-recurring activity.
        pub other_activity: Decimal,
        /// Total recurring activity that occurred.
        pub recurring_activity: Decimal,
        /// Expected recurring activity not yet occurred.
        pub recurring_remaining: Decimal,
        /// Expected recurring activity total.
        pub recurring_expected: Decimal,
        /// Inflow/outflow from uncategorized transactions.
        pub uncategorized: Decimal,
        /// Number of uncategorized transactions.
        pub uncategorized_count: u32,
        /// Inflow/outflow from uncategorized recurring transactions.
        pub uncategorized_recurring: Decimal,
    }

    /// Top-level totals breakdown.
    #[derive(Deserialize, Debug, Clone)]
    pub struct BudgetSummaryTotals {
        /// Total inflows.
        pub inflow: BudgetSummaryTotalsBreakdown,
        /// Total outflows.
        pub outflow: BudgetSummaryTotalsBreakdown,
    }

    /// Budget summary response.
    #[derive(Deserialize, Debug, Clone)]
    pub struct BudgetSummary {
        /// True if range is aligned with the user's budget settings.
        pub aligned: bool,
        /// Category budgets and activity details.
        pub categories: Vec<BudgetSummaryCategory>,
        /// Top-level totals. Only present if include_totals was true.
        pub totals: Option<BudgetSummaryTotals>,
        /// Rollover pool details. Only present if include_rollover_pool was true.
        pub rollover_pool: Option<BudgetSummaryRolloverPool>,
    }
}
