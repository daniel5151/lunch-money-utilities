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
#[derive(serde::Deserialize, Clone, Debug)]
pub struct ManualAccount<E = String> {
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
    pub custom_metadata: Option<serde_json::Value>,
    /// If true, this account will not show up for assignment.
    pub exclude_from_transactions: bool,
    /// The name of the user who created the account.
    pub created_by_name: String,
    /// Date/time the account was created.
    pub created_at: jiff::Timestamp,
    /// Date/time the account was last updated.
    pub updated_at: jiff::Timestamp,
}

/// Response payload containing a list of manual accounts.
#[derive(serde::Deserialize, Debug)]
pub struct ManualAccountsResponse {
    /// List of manual accounts.
    pub manual_accounts: Vec<ManualAccount>,
}
