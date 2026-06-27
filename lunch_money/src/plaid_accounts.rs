//! Synced Plaid account schemas.

/// Query parameters for Plaid accounts.
pub mod query_params {
    use serde::Serialize;

    use crate::core::PlaidAccountId;

    /// Query parameters to trigger a manual fetch from Plaid.
    #[derive(bon::Builder, Serialize, Debug, Clone, Default)]
    pub struct TriggerPlaidFetchQuery {
        /// Beginning of time period to fetch transactions for.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub start_date: Option<jiff::civil::Date>,
        /// End of time period to fetch transactions for.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub end_date: Option<jiff::civil::Date>,
        /// Specific ID of a Plaid account to fetch.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub id: Option<PlaidAccountId>,
    }
}

/// JSON schemas for Plaid accounts.
pub mod schemas {
    use rust_decimal::Decimal;
    use serde::Deserialize;
    use serde::Serialize;

    use crate::core::Currency;
    use crate::core::PlaidAccountId;

    /// Current status of a synced Plaid account.
    #[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
    #[serde(rename_all = "lowercase")]
    pub enum PlaidAccountStatus {
        /// Account is actively syncing.
        Active,
        /// Account marked inactive.
        Inactive,
        /// Account marked closed.
        Closed,
        /// Account deactivated during setup.
        Deactivated,
        /// Account linked but no longer found.
        #[serde(rename = "not found")]
        NotFound,
        /// Account not supported by Plaid.
        #[serde(rename = "not supported")]
        NotSupported,
        /// Account needs to be relinked.
        Relink,
        /// Account awaiting first import.
        Syncing,
        /// Connection revoked by Plaid.
        Revoked,
        /// Connection in error state.
        Error,
    }

    /// Information about a synced Plaid account.
    #[derive(Deserialize, Debug, Clone)]
    pub struct PlaidAccount {
        /// Unique identifier of the Plaid account.
        pub id: PlaidAccountId,
        /// Unique identifier of the Plaid item/connection.
        pub plaid_item_id: Option<String>,
        /// Date the account was first linked.
        pub date_linked: jiff::civil::Date,
        /// Name of the user who linked the account.
        pub linked_by_name: String,
        /// Institution name of the account set by Plaid.
        pub name: String,
        /// Display name of the account.
        pub display_name: Option<String>,
        /// Primary type of the account (e.g. credit, depository).
        #[serde(rename = "type")]
        pub account_type: String,
        /// Account subtype.
        pub subtype: Option<String>,
        /// Mask digits (last 3-4 digits).
        pub mask: Option<String>,
        /// Name of institution holding the account.
        pub institution_name: String,
        /// Current status of the account.
        pub status: PlaidAccountStatus,
        /// If true, imported transactions cannot be modified by the user.
        pub allow_transaction_modifications: bool,
        /// Credit limit, if applicable.
        pub limit: Option<Decimal>,
        /// Current balance of the account.
        #[serde(with = "rust_decimal::serde::str")]
        pub balance: Decimal,
        /// Currency of the balance.
        pub currency: Currency,
        /// Balance converted to user's primary currency.
        pub to_base: Decimal,
        /// Date balance was last updated.
        pub balance_last_update: Option<jiff::Timestamp>,
        /// Earliest date allowed for importing transactions.
        pub import_start_date: Option<jiff::civil::Date>,
        /// Timestamp of last import of new data.
        pub last_import: Option<jiff::Timestamp>,
        /// Timestamp of last successful request for updated data.
        pub last_fetch: Option<jiff::Timestamp>,
        /// Timestamp of last successful connection with institution.
        pub plaid_last_successful_update: Option<jiff::Timestamp>,
    }

    /// Response payload containing all synced Plaid accounts.
    #[derive(Deserialize, Debug)]
    pub struct PlaidAccountsResponse {
        /// List of Plaid accounts.
        pub plaid_accounts: Vec<PlaidAccount>,
    }
}
