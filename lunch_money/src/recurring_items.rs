//! Recurring items definitions.

/// Query parameters for recurring items.
pub mod query_params {
    use serde::Serialize;

    /// Query parameters for listing recurring items.
    #[derive(Serialize, Debug, Clone, Default)]
    pub struct RecurringItemsQuery {
        /// Range start date for populating matches.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub start_date: Option<jiff::civil::Date>,
        /// Range end date for populating matches.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub end_date: Option<jiff::civil::Date>,
        /// Include suggested items.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub include_suggested: Option<bool>,
    }

    /// Query parameters for a single recurring item.
    #[derive(Serialize, Debug, Clone, Default)]
    pub struct RecurringItemQuery {
        /// Range start date for populating matches.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub start_date: Option<jiff::civil::Date>,
        /// Range end date for populating matches.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub end_date: Option<jiff::civil::Date>,
    }
}

/// JSON schemas for recurring items.
pub mod schemas {
    use crate::core::CategoryId;
    use crate::core::Currency;
    use crate::core::ManualAccountId;
    use crate::core::PlaidAccountId;
    use crate::core::RecurringId;
    use crate::core::TransactionId;
    use crate::core::UserId;
    use rust_decimal::Decimal;
    use serde::Deserialize;
    use serde::Serialize;

    /// Status of a recurring item.
    #[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
    #[serde(rename_all = "lowercase")]
    pub enum RecurringItemStatus {
        /// Suggested by the system, not yet reviewed or applied.
        Suggested,
        /// Reviewed by the user, actively matching transactions.
        Reviewed,
    }

    /// Original source of the recurring item.
    #[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
    #[serde(rename_all = "lowercase")]
    pub enum RecurringItemSource {
        /// Created manually from the Recurring Items page.
        #[serde(rename = "manual")]
        Manual,
        /// Converted from a transaction.
        #[serde(rename = "transaction")]
        Transaction,
        /// Automatically created on transaction import.
        #[serde(rename = "system")]
        System,
    }

    /// The unit of time for the recurring item's cadence.
    #[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
    #[serde(rename_all = "lowercase")]
    pub enum RecurringGranularity {
        /// Daily recurrence.
        Day,
        /// Weekly recurrence.
        Week,
        /// Monthly recurrence.
        Month,
        /// Yearly recurrence.
        Year,
    }

    /// Criteria used to identify matching transactions.
    #[derive(Deserialize, Debug, Clone)]
    pub struct TransactionCriteria {
        /// Start date for matching.
        pub start_date: Option<jiff::civil::Date>,
        /// End date for matching.
        pub end_date: Option<jiff::civil::Date>,
        /// Unit of time defining the cadence.
        pub granularity: RecurringGranularity,
        /// Cadence quantity.
        pub quantity: u32,
        /// Anchor date to derive expected occurrence dates.
        pub anchor_date: jiff::civil::Date,
        /// Expected payee name.
        pub payee: Option<String>,
        /// Expected amount.
        #[serde(with = "rust_decimal::serde::str")]
        pub amount: Decimal,
        /// Converted amount in primary currency.
        pub to_base: Decimal,
        /// Currency code.
        pub currency: Currency,
        /// Synced account ID filter, if applicable.
        pub plaid_account_id: Option<PlaidAccountId>,
        /// Manual account ID filter, if applicable.
        pub manual_account_id: Option<ManualAccountId>,
    }

    /// Overrides applied to matching transactions.
    #[derive(Deserialize, Debug, Clone)]
    pub struct RecurringOverrides {
        /// Overridden payee.
        pub payee: Option<String>,
        /// Overridden notes.
        pub notes: Option<String>,
        /// Overridden category ID.
        pub category_id: Option<CategoryId>,
    }

    /// Represents a found transaction matching a recurring schedule.
    #[derive(Deserialize, Debug, Clone)]
    pub struct FoundTransactionMatch {
        /// Date of the transaction.
        pub date: jiff::civil::Date,
        /// ID of the matching transaction.
        pub transaction_id: TransactionId,
    }

    /// Details of expected, found, and missing occurrences for a recurring item.
    #[derive(Deserialize, Debug, Clone)]
    pub struct RecurringMatches {
        /// Request start date.
        pub request_start_date: jiff::civil::Date,
        /// Request end date.
        pub request_end_date: jiff::civil::Date,
        /// Dates of expected occurrences.
        pub expected_occurrence_dates: Vec<jiff::civil::Date>,
        /// List of actual matching transactions.
        pub found_transactions: Vec<FoundTransactionMatch>,
        /// Dates expected but missing matching transactions.
        pub missing_transaction_dates: Vec<jiff::civil::Date>,
    }

    /// Details of a recurring transaction definition.
    #[derive(Deserialize, Debug, Clone)]
    pub struct RecurringItem {
        /// Unique identifier of the recurring item.
        pub id: RecurringId,
        /// Description of the item.
        pub description: Option<String>,
        /// Status of the item.
        pub status: RecurringItemStatus,
        /// Criteria to match transactions.
        pub transaction_criteria: TransactionCriteria,
        /// Overrides to apply to matched transactions.
        pub overrides: RecurringOverrides,
        /// Matches within a given query range (only populated for Reviewed status).
        pub matches: Option<RecurringMatches>,
        /// ID of user who created the item.
        pub created_by: UserId,
        /// Date created.
        pub created_at: jiff::Timestamp,
        /// Date updated.
        pub updated_at: jiff::Timestamp,
        /// The creation source.
        pub source: Option<RecurringItemSource>,
    }

    /// Response payload containing list of recurring items.
    #[derive(Deserialize, Debug)]
    pub struct RecurringItemsResponse {
        /// List of recurring items.
        pub recurring_items: Vec<RecurringItem>,
    }
}
