//! Manual account schema models, types, and responses.

/// JSON schemas for manual accounts.
pub mod schemas {
    use crate::core::Currency;
    use crate::core::ManualAccountId;
    use rust_decimal::Decimal;
    use serde::Deserialize;
    use serde::Serialize;

    /// Represents the date or date-time at which a manual account's balance was last updated.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum BalanceAsOf {
        /// Date only (e.g. YYYY-MM-DD).
        Date(jiff::civil::Date),
        /// Complete date-time / timestamp.
        Timestamp(jiff::Timestamp),
    }

    impl<'de> serde::Deserialize<'de> for BalanceAsOf {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            let s = String::deserialize(deserializer)?;
            if let Ok(ts) = s.parse::<jiff::Timestamp>() {
                return Ok(Self::Timestamp(ts));
            }
            if let Ok(date) = s.parse::<jiff::civil::Date>() {
                return Ok(Self::Date(date));
            }
            Err(serde::de::Error::custom(format!(
                "invalid date or timestamp format for balance_as_of: '{}'",
                s
            )))
        }
    }

    impl serde::Serialize for BalanceAsOf {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: serde::Serializer,
        {
            match self {
                Self::Date(date) => date.serialize(serializer),
                Self::Timestamp(ts) => ts.serialize(serializer),
            }
        }
    }

    /// The status of a manual or synced account.
    #[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
    #[serde(rename_all = "lowercase")]
    pub enum AccountStatus {
        /// The account is active.
        Active,
        /// The account has been closed.
        Closed,
    }

    impl std::fmt::Display for AccountStatus {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::Active => write!(f, "active"),
                Self::Closed => write!(f, "closed"),
            }
        }
    }

    /// The primary type of an account.
    #[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
    #[serde(rename_all = "lowercase")]
    pub enum AccountType {
        /// Cash account.
        Cash,
        /// Credit account (liability).
        Credit,
        /// Cryptocurrency asset account.
        Cryptocurrency,
        /// Employee compensation account.
        #[serde(rename = "employee compensation")]
        EmployeeCompensation,
        /// Investment asset account.
        Investment,
        /// Loan liability account.
        Loan,
        /// Other liability account.
        #[serde(rename = "other liability")]
        OtherLiability,
        /// Other asset account.
        #[serde(rename = "other asset")]
        OtherAsset,
        /// Real estate asset account.
        #[serde(rename = "real estate")]
        RealEstate,
        /// Vehicle asset account.
        Vehicle,
    }

    /// Request payload to update manual account details.
    #[derive(Serialize, Clone, Debug)]
    pub struct UpdateManualAccountObject {
        /// The new balance of the account.
        pub balance: Decimal,
    }

    /// A manually managed account in Lunch Money.
    #[derive(serde::Deserialize, serde::Serialize, Clone, Debug)]
    pub struct ManualAccount<E = String, M = serde_json::Value> {
        /// Unique identifier of the manual account.
        pub id: ManualAccountId,
        /// Name of the manual account.
        pub name: String,
        /// Name of institution holding the account.
        pub institution_name: Option<String>,
        /// Optional display name.
        pub display_name: Option<String>,
        /// Primary type of the account.
        #[serde(rename = "type")]
        pub account_type: AccountType,
        /// Optional account subtype.
        pub subtype: Option<String>,
        /// Current balance of the manual account.
        #[serde(with = "rust_decimal::serde::str")]
        pub balance: Decimal,
        /// Currency of the manual account balance.
        pub currency: Currency,
        /// The balance converted to the user's primary currency.
        pub to_base: Decimal,
        /// Date balance was last updated.
        pub balance_as_of: BalanceAsOf,
        /// Account status (active or closed).
        pub status: AccountStatus,
        /// The date this account was closed.
        pub closed_on: Option<jiff::civil::Date>,
        /// An optional external ID.
        pub external_id: Option<E>,
        /// User defined JSON data.
        pub custom_metadata: Option<M>,
        /// If true, this account will not show up for assignment.
        pub exclude_from_transactions: bool,
        /// The name of the user who created the account.
        pub created_by_name: String,
        /// Date/time the account was created.
        pub created_at: jiff::Timestamp,
        /// Date/time the account was last updated.
        pub updated_at: jiff::Timestamp,
    }

    /// Request payload for creating a new manual account.
    #[derive(bon::Builder, Serialize, Clone, Debug)]
    pub struct CreateManualAccountPayload<E = String, M = serde_json::Value> {
        /// Name of the manual account.
        pub name: String,
        /// Primary type of the account.
        #[serde(rename = "type")]
        pub account_type: AccountType,
        /// Current balance of the manual account.
        #[serde(with = "rust_decimal::serde::str")]
        pub balance: Decimal,
        /// Name of institution holding the account.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub institution_name: Option<String>,
        /// Optional display name.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub display_name: Option<String>,
        /// Optional account subtype.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub subtype: Option<String>,
        /// Date balance was last updated.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub balance_as_of: Option<BalanceAsOf>,
        /// Currency of the manual account balance.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub currency: Option<Currency>,
        /// Account status (active or closed).
        #[serde(skip_serializing_if = "Option::is_none")]
        pub status: Option<AccountStatus>,
        /// The date this account was closed.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub closed_on: Option<jiff::civil::Date>,
        /// An optional external ID.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub external_id: Option<E>,
        /// User defined JSON data.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub custom_metadata: Option<M>,
        /// If true, this account will not show up for assignment.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub exclude_from_transactions: Option<bool>,
    }

    /// Request payload for updating an existing manual account.
    #[derive(bon::Builder, Serialize, Clone, Debug, Default)]
    pub struct UpdateManualAccountPayload<E = String, M = serde_json::Value> {
        /// New name of the manual account.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub name: Option<String>,
        /// New institution name.
        #[serde(skip_serializing_if = "Option::is_none")]
        #[expect(clippy::option_option)]
        pub institution_name: Option<Option<String>>,
        /// New display name.
        #[serde(skip_serializing_if = "Option::is_none")]
        #[expect(clippy::option_option)]
        pub display_name: Option<Option<String>>,
        /// New type of the manual account.
        #[serde(rename = "type")]
        #[serde(skip_serializing_if = "Option::is_none")]
        pub account_type: Option<AccountType>,
        /// New subtype.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub subtype: Option<String>,
        /// New balance of the manual account.
        #[serde(skip_serializing_if = "Option::is_none")]
        #[serde(with = "rust_decimal::serde::str_option")]
        pub balance: Option<Decimal>,
        /// New currency.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub currency: Option<Currency>,
        /// Date balance was last updated.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub balance_as_of: Option<BalanceAsOf>,
        /// New status.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub status: Option<AccountStatus>,
        /// New closed date.
        #[serde(skip_serializing_if = "Option::is_none")]
        #[expect(clippy::option_option)]
        pub closed_on: Option<Option<jiff::civil::Date>>,
        /// New external ID.
        #[serde(skip_serializing_if = "Option::is_none")]
        #[expect(clippy::option_option)]
        pub external_id: Option<Option<E>>,
        /// New custom metadata.
        #[serde(skip_serializing_if = "Option::is_none")]
        #[expect(clippy::option_option)]
        pub custom_metadata: Option<Option<M>>,
        /// Exclude from transaction assignment.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub exclude_from_transactions: Option<bool>,
    }

    /// Response payload containing a list of manual accounts.
    #[derive(serde::Deserialize, Debug)]
    pub struct ManualAccountsResponse<E = String, M = serde_json::Value> {
        /// List of manual accounts.
        pub manual_accounts: Vec<ManualAccount<E, M>>,
    }
}
