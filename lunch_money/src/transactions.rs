//! Transaction schema models, filters, and payloads.

/// Query parameters for transaction endpoints.
pub mod query_params {
    use crate::core::CategoryId;
    use crate::core::ManualAccountId;
    use crate::core::PlaidAccountId;
    use crate::core::RecurringId;
    use crate::core::TagId;
    use crate::transactions::schemas::TransactionStatus;
    use serde::Serialize;

    /// Query parameters for fetching transactions.
    #[derive(bon::Builder, Serialize, Debug, Clone, Default)]
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
        /// Filter transactions updated after this timestamp (ISO 8601 date or date-time).
        #[serde(skip_serializing_if = "Option::is_none")]
        pub updated_since: Option<String>,
        /// Filter transactions created after this timestamp (ISO 8601 date or date-time).
        #[serde(skip_serializing_if = "Option::is_none")]
        pub created_since: Option<String>,
        /// Filter by Plaid account ID, or set to `PlaidAccountId(0)` to omit Plaid transactions.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub plaid_account_id: Option<PlaidAccountId>,
        /// Filter by Recurring Item ID.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub recurring_id: Option<RecurringId>,
        /// Filter by Category ID, or set to `CategoryId(0)` for uncategorized.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub category_id: Option<CategoryId>,
        /// If true, returns only transaction groups.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub is_group_parent: Option<bool>,
        /// Filter by transaction status.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub status: Option<TransactionStatus>,
        /// Filter by pending status.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub is_pending: Option<bool>,
        /// If true, include pending transactions in results.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub include_pending: Option<bool>,
        /// If true, include child transactions of groups/splits in the `children` array.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub include_children: Option<bool>,
        /// If true, populate the `files` array with transaction attachments.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub include_files: Option<bool>,
        /// Pagination offset.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub offset: Option<u32>,
    }
}

/// JSON schemas for transaction endpoints.
pub mod schemas {
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
    pub struct ChildTransaction<M = serde_json::Value, E = String> {
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
        #[serde(default)]
        pub is_split_parent: Option<bool>,
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
        pub custom_metadata: Option<M>,
        /// A list of objects that describe any attachments to the transaction.
        #[serde(default = "Vec::new")]
        pub files: Vec<TransactionAttachment>,
        /// Unique identifier for associated recurring item.
        pub recurring_id: Option<RecurringId>,
    }

    /// A Lunch Money transaction.
    #[derive(Deserialize, Clone, Debug)]
    pub struct Transaction<M = serde_json::Value, E = String> {
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
        pub children: Vec<ChildTransaction<M, E>>,
        /// System set metadata from Plaid sync.
        pub plaid_metadata: Option<serde_json::Value>,
        /// Optional custom JSON metadata.
        pub custom_metadata: Option<M>,
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
    pub struct TransactionsResponse<M = serde_json::Value, E = String> {
        /// List of transaction objects.
        pub transactions: Vec<Transaction<M, E>>,
        /// Indicates whether more transactions are available beyond the current page.
        pub has_more: bool,
    }

    /// Request payload for inserting new transactions.
    #[derive(Serialize, Debug)]
    pub struct InsertPayload<M = serde_json::Value, E = String> {
        /// List of transaction objects to insert.
        pub transactions: Vec<InsertObject<M, E>>,
    }

    /// Object representing a transaction to be inserted.
    #[derive(bon::Builder, Serialize, Deserialize, Clone, Debug)]
    pub struct InsertObject<M = serde_json::Value, E = String> {
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
        pub custom_metadata: Option<M>,
    }

    /// Request payload for updating transactions.
    #[derive(Serialize, Debug)]
    pub struct UpdatePayload<M = serde_json::Value, E = String> {
        /// List of transaction objects to update.
        pub transactions: Vec<UpdateObject<M, E>>,
    }

    /// Object representing updates to make on an existing transaction.
    #[derive(bon::Builder, Serialize, Clone, Debug)]
    pub struct UpdateObject<M = serde_json::Value, E = String> {
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
        pub custom_metadata: Option<M>,
        /// Status of the transaction (reviewed or unreviewed).
        #[serde(skip_serializing_if = "Option::is_none")]
        pub status: Option<TransactionStatus>,
        /// Unique identifier of the associated recurring item. Set to null to clear.
        #[serde(skip_serializing_if = "Option::is_none")]
        #[expect(clippy::option_option)]
        pub recurring_id: Option<Option<RecurringId>>,
    }

    /// Response payload returned by a transaction insertion request.
    #[derive(Deserialize, Debug)]
    pub struct InsertTransactionsResponse<M = serde_json::Value, E = String> {
        /// List of successfully inserted transaction objects.
        pub transactions: Vec<Transaction<M, E>>,
        /// List of transactions that were skipped because they duplicate existing external IDs.
        pub skipped_duplicates: Vec<SkippedExistingExternalIdObject<M, E>>,
    }

    /// Object describing a transaction that was skipped during insertion.
    #[derive(Deserialize, Clone, Debug)]
    pub struct SkippedExistingExternalIdObject<M = serde_json::Value, E = String> {
        /// The reason the transaction was skipped (e.g. duplicate_external_id).
        pub reason: String,
        /// The index of the skipped transaction in the original request.
        pub request_transactions_index: usize,
        /// The ID of the existing transaction that this duplicate matched.
        pub existing_transaction_id: TransactionId,
        /// The original transaction that was skipped.
        pub request_transaction: Option<InsertObject<M, E>>,
    }

    /// Request payload for deleting transactions.
    #[derive(Serialize, Debug)]
    pub struct DeletePayload {
        /// List of transaction IDs to delete.
        pub ids: Vec<TransactionId>,
    }

    /// Object representing a split transaction child.
    #[derive(bon::Builder, Serialize, Deserialize, Clone, Debug)]
    pub struct SplitTransactionObject {
        /// Individual amount of split.
        #[serde(with = "rust_decimal::serde::str")]
        pub amount: Decimal,
        /// The payee for the child transaction. Will inherit original payee from parent if not defined.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub payee: Option<String>,
        /// Date of the transaction. Will inherit from parent if not defined.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub date: Option<jiff::civil::Date>,
        /// Unique identifier for associated category. Will inherit from parent if not defined.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub category_id: Option<CategoryId>,
        /// The IDs of any tags to apply.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub tag_ids: Option<Vec<TagId>>,
        /// Notes for the child transaction. Will inherit from parent if not defined.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub notes: Option<String>,
    }

    /// Request payload for splitting a transaction.
    #[derive(Serialize, Clone, Debug)]
    pub struct SplitTransactionPayload {
        /// List of child transactions to create.
        pub child_transactions: Vec<SplitTransactionObject>,
    }

    /// Request payload for creating a transaction group.
    #[derive(bon::Builder, Serialize, Clone, Debug)]
    pub struct CreateTransactionGroupPayload {
        /// List of existing transaction IDs to group.
        pub ids: Vec<TransactionId>,
        /// Date for the new grouped transaction.
        pub date: jiff::civil::Date,
        /// The payee for the new grouped transaction.
        pub payee: String,
        /// The ID of an existing category to assign to the grouped transaction.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub category_id: Option<CategoryId>,
        /// Notes for the grouped transaction.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub notes: Option<String>,
        /// Status of the grouped transaction.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub status: Option<TransactionStatus>,
        /// A list of IDs for the tags associated with the grouped transaction.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub tag_ids: Option<Vec<TagId>>,
    }

    /// Response returned by the signed attachment URL endpoint.
    #[derive(Deserialize, Clone, Debug)]
    pub struct AttachmentUrlResponse {
        /// The signed URL to download the file attachment.
        pub url: String,
        /// The date and time the signed URL will expire.
        pub expires_at: jiff::Timestamp,
    }
}
