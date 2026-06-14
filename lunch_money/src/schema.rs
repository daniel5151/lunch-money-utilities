//! Data structures representing schemas returned by or sent to the Lunch Money API.

use rust_decimal::Decimal;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::Serializer;
use std::fmt;

/// A case-insensitive wrapper around a currency code (e.g. USD, EUR, GBP)
/// that always normalizes to uppercase for internal comparisons and hashing,
/// but serializes to lowercase for compatibility with the Lunch Money API.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct Currency(String);

impl Currency {
    /// Creates a new `Currency` instance, converting the input to uppercase.
    pub fn new(code: impl AsRef<str>) -> Self {
        Self(code.as_ref().to_ascii_uppercase())
    }

    /// Returns the uppercase string representation of the currency.
    pub fn to_uppercase(&self) -> String {
        self.0.clone()
    }

    /// Returns the lowercase string representation of the currency.
    pub fn to_lowercase(&self) -> String {
        self.0.to_ascii_lowercase()
    }

    /// Returns a reference to the underlying normalized uppercase string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Currency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl<'de> Deserialize<'de> for Currency {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(Self::new(s))
    }
}

impl Serialize for Currency {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // Serialize to lowercase for Lunch Money compatibility
        serializer.serialize_str(&self.to_lowercase())
    }
}

macro_rules! define_id {
    ($name:ident, $doc:expr) => {
        #[doc = $doc]
        #[derive(
            Debug,
            Clone,
            Copy,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
            Hash,
            serde::Serialize,
            serde::Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(pub u64);

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl From<u64> for $name {
            fn from(val: u64) -> Self {
                Self(val)
            }
        }
    };
}

define_id!(
    TransactionId,
    "Type-safe identifier for Lunch Money transactions."
);
define_id!(
    CategoryId,
    "Type-safe identifier for Lunch Money categories."
);
define_id!(
    ManualAccountId,
    "Type-safe identifier for Lunch Money manual accounts."
);
define_id!(
    PlaidAccountId,
    "Type-safe identifier for Lunch Money Plaid accounts."
);
define_id!(TagId, "Type-safe identifier for Lunch Money tags.");
define_id!(
    RecurringId,
    "Type-safe identifier for Lunch Money recurring items."
);
define_id!(
    AttachmentId,
    "Type-safe identifier for transaction attachments."
);
define_id!(UserId, "Type-safe identifier for a Lunch Money user.");

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

/// Status of a Lunch Money transaction.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TransactionStatus {
    /// Transaction has been reviewed by the user.
    Reviewed,
    /// Transaction has not been reviewed by the user.
    Unreviewed,
    /// Transaction is pending deletion.
    #[serde(rename = "delete_pending")]
    DeletePending,
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

/// Response payload containing a list of transactions.
#[derive(Deserialize)]
pub struct TransactionsResponse<T = (), E = String> {
    /// List of transaction objects.
    pub transactions: Vec<Transaction<T, E>>,
}

/// Source of a Lunch Money transaction.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TransactionSource {
    /// Added via the POST /transactions API
    Api,
    /// Added via a CSV Import
    Csv,
    /// Created via "Add to Cash" button on Transactions page
    Manual,
    /// Originally in an account that was merged
    Merge,
    /// Came from Plaid financial institution sync
    Plaid,
    /// Created from Recurring page
    Recurring,
    /// Created by a split rule
    Rule,
    /// Created by splitting another transaction
    Split,
    /// Legacy value replaced by csv or manual
    User,
}

/// An attachment associated with a Lunch Money transaction.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TransactionAttachment {
    /// The unique identifier of the attachment.
    pub id: AttachmentId,
    /// The id of the user who uploaded the attachment.
    pub uploaded_by: UserId,
    /// The name of the file.
    pub name: String,
    /// The MIME type of the file.
    #[serde(rename = "type")]
    pub mime_type: String,
    /// The size of the file in kilobytes.
    pub size: u64,
    /// Optional notes about the attachment.
    pub notes: Option<String>,
    /// The date and time when the attachment was created.
    pub created_at: jiff::Timestamp,
}

/// A Lunch Money transaction that is a child of a split or group.
#[derive(Deserialize, Clone, Debug)]
pub struct ChildTransaction<T = (), E = String> {
    /// System-created unique identifier for the transaction.
    pub id: TransactionId,
    /// Date of the transaction.
    pub date: jiff::civil::Date,
    /// Amount of the transaction. Positive values indicate a debit, negative indicate a credit.
    pub amount: Decimal,
    /// Currency of the transaction.
    pub currency: Currency,
    /// The amount converted to the user's primary currency.
    pub to_base: Decimal,
    /// Payee name.
    pub payee: String,
    /// Original payee name from the source.
    pub original_name: Option<String>,
    /// Unique identifier of the associated category.
    pub category_id: Option<CategoryId>,
    /// Any notes associated with the transaction.
    pub notes: Option<String>,
    /// Status of the transaction (e.g. reviewed, unreviewed).
    pub status: TransactionStatus,
    /// Denotes if the transaction is pending.
    pub is_pending: bool,
    /// The date and time of when the transaction was created.
    pub created_at: jiff::Timestamp,
    /// The date and time of when the transaction was last updated.
    pub updated_at: jiff::Timestamp,
    /// Denotes whether this transaction is the parent of a split.
    pub is_split_parent: bool,
    /// A transaction ID if this is a split transaction.
    pub split_parent_id: Option<TransactionId>,
    /// Denotes whether this transaction represents a group of transactions.
    pub is_group_parent: bool,
    /// Denotes the ID of the group parent transaction if this is grouped.
    pub group_parent_id: Option<TransactionId>,
    /// Associated manual account ID.
    pub manual_account_id: Option<ManualAccountId>,
    /// The unique identifier of the plaid account associated with this transaction.
    pub plaid_account_id: Option<PlaidAccountId>,
    /// A list of tag_ids for the tags associated with this transaction.
    pub tag_ids: Vec<TagId>,
    /// Source of the transaction.
    pub source: Option<TransactionSource>,
    /// Optional user-defined external ID.
    pub external_id: Option<E>,
    /// System set metadata from Plaid sync.
    pub plaid_metadata: Option<serde_json::Value>,
    /// Optional custom JSON metadata.
    pub custom_metadata: Option<T>,
    /// A list of objects that describe any attachments to the transaction.
    #[serde(default = "Vec::new")]
    pub files: Vec<TransactionAttachment>,
    /// Unique identifier for associated recurring item.
    pub recurring_id: Option<RecurringId>,
}

/// A Lunch Money transaction.
#[derive(Deserialize, Clone, Debug)]
pub struct Transaction<T = (), E = String> {
    /// System-created unique identifier for the transaction.
    pub id: TransactionId,
    /// Date of the transaction.
    pub date: jiff::civil::Date,
    /// Amount of the transaction. Positive values indicate a debit, negative indicate a credit.
    pub amount: Decimal,
    /// Currency of the transaction.
    pub currency: Currency,
    /// The amount converted to the user's primary currency.
    pub to_base: Decimal,
    /// Payee name.
    pub payee: String,
    /// Original payee name from the source.
    pub original_name: Option<String>,
    /// Any notes associated with the transaction.
    pub notes: Option<String>,
    /// Optional user-defined external ID.
    pub external_id: Option<E>,
    /// Associated manual account ID.
    pub manual_account_id: Option<ManualAccountId>,
    /// The unique identifier of the plaid account associated with this transaction.
    pub plaid_account_id: Option<PlaidAccountId>,
    /// A list of tag_ids for the tags associated with this transaction.
    pub tag_ids: Vec<TagId>,
    /// Status of the transaction (e.g. reviewed, unreviewed).
    pub status: TransactionStatus,
    /// Denotes if the transaction is pending.
    pub is_pending: bool,
    /// The date and time of when the transaction was created.
    pub created_at: jiff::Timestamp,
    /// The date and time of when the transaction was last updated.
    pub updated_at: jiff::Timestamp,
    /// Denotes whether this transaction is the parent of a split.
    pub is_split_parent: Option<bool>,
    /// A transaction ID if this is a split transaction.
    pub split_parent_id: Option<TransactionId>,
    /// Denotes whether this transaction represents a group of transactions.
    pub is_group_parent: bool,
    /// Denotes the ID of the group parent transaction if this is grouped.
    pub group_parent_id: Option<TransactionId>,
    /// Unique identifier of the associated category.
    pub category_id: Option<CategoryId>,
    /// Exists only for transactions which are the parent of a split transaction or for transaction groups.
    #[serde(default = "Vec::new")]
    pub children: Vec<ChildTransaction<T, E>>,
    /// System set metadata from Plaid sync.
    pub plaid_metadata: Option<serde_json::Value>,
    /// Optional custom JSON metadata.
    pub custom_metadata: Option<T>,
    /// A list of objects that describe any attachments to the transaction.
    #[serde(default = "Vec::new")]
    pub files: Vec<TransactionAttachment>,
    /// Source of the transaction.
    pub source: Option<TransactionSource>,
    /// Unique identifier for associated recurring item.
    pub recurring_id: Option<RecurringId>,
}

/// Request payload for inserting new transactions.
#[derive(Serialize, Debug)]
pub struct InsertPayload<T = (), E = String> {
    /// List of transaction objects to insert.
    pub transactions: Vec<InsertObject<T, E>>,
}

/// Object representing a transaction to be inserted.
#[derive(Serialize, Clone, Debug)]
pub struct InsertObject<T = (), E = String> {
    /// Date of the transaction.
    pub date: jiff::civil::Date,
    /// Transaction amount.
    pub amount: Decimal,
    /// Currency of the transaction.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub currency: Option<Currency>,
    /// Payee name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payee: Option<String>,
    /// Original payee name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_name: Option<String>,
    /// Transaction notes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// User-defined external ID (must be unique for the manual account).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_id: Option<E>,
    /// Unique identifier for the manually managed account.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manual_account_id: Option<ManualAccountId>,
    /// The unique identifier of the plaid account associated with this transaction.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plaid_account_id: Option<PlaidAccountId>,
    /// Unique identifier for associated recurring item.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recurring_id: Option<RecurringId>,
    /// Transaction status (reviewed or unreviewed).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<TransactionStatus>,
    /// Optional list of tag IDs to associate with this transaction.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag_ids: Option<Vec<TagId>>,
    /// Optional category ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category_id: Option<CategoryId>,
    /// Optional custom JSON metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_metadata: Option<T>,
}

/// Response payload containing a list of tags.
#[derive(Deserialize, Debug)]
pub struct TagsResponse {
    /// List of tag objects.
    pub tags: Vec<Tag>,
}

/// A Lunch Money tag.
#[derive(Deserialize, Clone, Debug)]
pub struct Tag {
    /// Unique identifier for the tag.
    pub id: TagId,
    /// Name of the tag.
    pub name: String,
    /// Description of the tag.
    pub description: Option<String>,
    /// The text color of the tag.
    pub text_color: Option<String>,
    /// The background color of the tag.
    pub background_color: Option<String>,
    /// The date and time of when the tag was created.
    pub created_at: jiff::Timestamp,
    /// The date and time of when the tag was last updated.
    pub updated_at: jiff::Timestamp,
    /// If true, the tag is archived and hidden in the app UI.
    pub archived: bool,
    /// The date and time of when the tag was last archived.
    pub archived_at: Option<jiff::Timestamp>,
}

/// Request payload for creating a new tag.
#[derive(Serialize, Debug)]
pub struct CreateTagPayload {
    /// Name of the tag (between 1 and 100 characters).
    pub name: String,
    /// Description of the tag (up to 200 characters).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Request payload for updating transactions.
#[derive(Serialize, Debug)]
pub struct UpdatePayload<T = (), E = String> {
    /// List of transaction objects to update.
    pub transactions: Vec<UpdateObject<T, E>>,
}

/// Object representing updates to make on an existing transaction.
#[derive(Serialize, Clone, Debug)]
pub struct UpdateObject<T = (), E = String> {
    /// System defined unique identifier of the transaction.
    pub id: TransactionId,
    /// Date of the transaction.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<jiff::civil::Date>,
    /// Transaction amount.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount: Option<Decimal>,
    /// Currency of the transaction.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub currency: Option<Currency>,
    /// Payee name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payee: Option<String>,
    /// Unique identifier of the category for this transaction. Set to null to clear.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[expect(clippy::option_option)]
    pub category_id: Option<Option<CategoryId>>,
    /// Transaction notes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// The unique identifier of the manual account associated with this transaction. Set to null to clear.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[expect(clippy::option_option)]
    pub manual_account_id: Option<Option<ManualAccountId>>,
    /// The unique identifier of the plaid account associated with this transaction. Set to null to clear.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[expect(clippy::option_option)]
    pub plaid_account_id: Option<Option<PlaidAccountId>>,
    /// A list of tag_ids for the tags associated with this transaction. If set, overwrites existing tags.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag_ids: Option<Vec<TagId>>,
    /// Optional list of tag IDs to add.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub additional_tag_ids: Option<Vec<TagId>>,
    /// Optional external ID update.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[expect(clippy::option_option)]
    pub external_id: Option<Option<E>>,
    /// Optional custom JSON metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_metadata: Option<T>,
    /// Status of the transaction (reviewed or unreviewed).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<TransactionStatus>,
}

/// Response payload returned by a transaction insertion request.
#[derive(Deserialize, Debug)]
pub struct InsertTransactionsResponse<T = (), E = String> {
    /// List of successfully inserted transaction objects.
    pub transactions: Vec<Transaction<T, E>>,
    /// List of transactions that were skipped because they duplicate existing external IDs.
    pub skipped_duplicates: Vec<SkippedExistingExternalIdObject>,
}

/// Object describing a transaction that was skipped during insertion.
#[derive(Deserialize, Clone, Debug)]
pub struct SkippedExistingExternalIdObject {
    /// The reason the transaction was skipped (e.g. duplicate_external_id).
    pub reason: String,
    /// The index of the skipped transaction in the original request.
    pub request_transactions_index: usize,
    /// The ID of the existing transaction that this duplicate matched.
    pub existing_transaction_id: TransactionId,
}

/// Request payload for deleting transactions.
#[derive(Serialize, Debug)]
pub struct DeletePayload {
    /// List of transaction IDs to delete.
    pub ids: Vec<TransactionId>,
}

/// Request payload to update manual account details.
#[derive(Serialize, Clone, Debug)]
pub struct UpdateManualAccountObject {
    /// The new balance of the account.
    pub balance: Decimal,
}

/// Response payload containing a list of categories.
#[derive(Deserialize, Debug)]
pub struct CategoriesResponse {
    /// List of category objects.
    pub categories: Vec<Category>,
}

/// A Lunch Money category.
#[derive(Deserialize, Clone, Debug)]
pub struct Category {
    /// System-defined unique ID for the category.
    pub id: CategoryId,
    /// Name of the category.
    pub name: String,
    /// The description of the category or `null` if not set.
    pub description: Option<String>,
    /// If true, the transactions in this category will be treated as income.
    pub is_income: bool,
    /// If true, the transactions in this category will be excluded from the budget.
    pub exclude_from_budget: bool,
    /// If true, the transactions in this category will be excluded from totals.
    pub exclude_from_totals: bool,
    /// The date and time of when the category was last updated.
    pub updated_at: jiff::Timestamp,
    /// The date and time of when the category was created.
    pub created_at: jiff::Timestamp,
    /// ID of the parent category group, if applicable.
    pub group_id: Option<CategoryId>,
    /// Whether this category is a group containing other categories.
    pub is_group: bool,
    /// Optional list of children categories (only populated for groups).
    pub children: Option<Vec<ChildCategory>>,
    /// Whether this category is archived.
    pub archived: bool,
    /// The date and time of when the category was last archived.
    pub archived_at: Option<jiff::Timestamp>,
    /// An integer specifying the position in which the category is displayed.
    pub order: Option<u32>,
    /// If true, the category is collapsed in the Lunch Money GUI.
    pub collapsed: bool,
}

/// A category that is a child of a category group.
#[derive(Deserialize, Clone, Debug)]
pub struct ChildCategory {
    /// Unique identifier for the category.
    pub id: CategoryId,
    /// Name of the category.
    pub name: String,
    /// The description of the category or `null` if not set.
    pub description: Option<String>,
    /// If true, the transactions in this category will be treated as income.
    pub is_income: bool,
    /// If true, the transactions in this category will be excluded from the budget.
    pub exclude_from_budget: bool,
    /// If true, the transactions in this category will be excluded from totals.
    pub exclude_from_totals: bool,
    /// The date and time of when the category was last updated.
    pub updated_at: jiff::Timestamp,
    /// The date and time of when the category was created.
    pub created_at: jiff::Timestamp,
    /// ID of the parent category group.
    pub group_id: Option<CategoryId>,
    /// Whether this category is archived.
    pub archived: bool,
    /// The date and time of when the category was last archived.
    pub archived_at: Option<jiff::Timestamp>,
    /// An index specifying the position in which the category is displayed.
    pub order: Option<u32>,
    /// If true, the category is collapsed in the Lunch Money GUI.
    pub collapsed: Option<bool>,
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
