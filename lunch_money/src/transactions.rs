use crate::core::AttachmentId;
use crate::core::CategoryId;
use crate::core::Currency;
use crate::core::ManualAccountId;
use crate::core::PlaidAccountId;
use crate::core::RecurringId;
use crate::core::TagId;
use crate::core::TransactionId;
use crate::core::UserId;
use rust_decimal::Decimal;
use serde::Deserialize;
use serde::Serialize;

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

/// Response payload containing a list of transactions.
#[derive(Deserialize)]
pub struct TransactionsResponse<T = (), E = String> {
    /// List of transaction objects.
    pub transactions: Vec<Transaction<T, E>>,
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

/// Query parameters for fetching transactions.
#[derive(serde::Serialize, Debug, Clone)]
pub struct TransactionQuery {
    /// Start date in ISO 8601 format (YYYY-MM-DD).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_date: Option<String>,
    /// End date in ISO 8601 format (YYYY-MM-DD).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_date: Option<String>,
    /// Unique identifier for the manual account.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manual_account_id: Option<ManualAccountId>,
    /// Maximum number of transactions to return.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    /// If true, include transactions that are children of a group.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_group_children: Option<bool>,
    /// If true, include parent transactions of splits.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_split_parents: Option<bool>,
    /// If true, include custom metadata in the response.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_metadata: Option<bool>,
    /// Filter transactions by tag ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag_id: Option<TagId>,
}
