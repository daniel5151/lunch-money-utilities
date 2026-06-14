//! A domain-specific wrapper around the generic `lunch_money` client.
//!
//! While the underlying `lunch_money` library is generic and relies heavily on
//! `Option` fields to support a variety of API use cases, the CLI application
//! requires a stricter, fully-specified schema (e.g., non-optional payees, currencies,
//! and manual account IDs).
//!
//! This module provides a strict `Client` wrapper and custom, non-optional types
//! (like `TransactionQuery`, `InsertObject`, and `UpdateObject`) to preserve downstream
//! developer ergonomics. The wrapper maps these strict types into the library's optional
//! equivalents at the boundary.

use crate::api::ExternalId;
use crate::metadata::LunchMoneyTxMetadata;
use lunch_money::Currency;
use rust_decimal::Decimal;

#[derive(Clone)]
pub struct Client(lunch_money::Client);

#[derive(serde::Serialize, Debug, Clone)]
pub struct TransactionQuery {
    pub start_date: String,
    pub end_date: String,
    pub manual_account_id: u64,
    pub limit: Option<u32>,
    pub include_group_children: Option<bool>,
    pub include_split_parents: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_metadata: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag_id: Option<u64>,
}

impl Client {
    pub fn new(http: reqwest::Client, api_key: String) -> Self {
        Self(lunch_money::Client::new(http, api_key))
    }

    pub async fn fetch_manual_accounts(&self) -> anyhow::Result<Vec<schema::ManualAccount>> {
        self.0.fetch_manual_accounts().await
    }

    pub async fn fetch_transactions(
        &self,
        query: &TransactionQuery,
    ) -> anyhow::Result<Vec<schema::Transaction>> {
        let lib_query = lunch_money::TransactionQuery {
            start_date: Some(query.start_date.clone()),
            end_date: Some(query.end_date.clone()),
            manual_account_id: Some(query.manual_account_id),
            limit: query.limit,
            include_group_children: query.include_group_children,
            include_split_parents: query.include_split_parents,
            include_metadata: query.include_metadata,
            tag_id: query.tag_id,
        };
        self.0.fetch_transactions(&lib_query).await
    }

    pub async fn fetch_transaction_by_id(
        &self,
        id: u64,
    ) -> anyhow::Result<Option<schema::Transaction>> {
        self.0.fetch_transaction_by_id(id).await
    }

    pub async fn fetch_categories(
        &self,
        format: Option<&str>,
    ) -> anyhow::Result<Vec<schema::Category>> {
        self.0.fetch_categories(format).await
    }

    pub async fn fetch_tags(&self) -> anyhow::Result<Vec<schema::Tag>> {
        self.0.fetch_tags().await
    }

    pub async fn create_tag(
        &self,
        name: &str,
        description: Option<&str>,
    ) -> anyhow::Result<schema::Tag> {
        self.0.create_tag(name, description).await
    }

    pub async fn insert_transactions(
        &self,
        txs: &[schema::InsertObject],
    ) -> anyhow::Result<schema::InsertTransactionsResponse> {
        let lib_txs: Vec<lunch_money::schema::InsertObject<LunchMoneyTxMetadata, ExternalId>> = txs
            .iter()
            .map(|tx| lunch_money::schema::InsertObject {
                date: tx.date,
                amount: tx.amount,
                currency: Some(tx.currency.clone()),
                payee: Some(tx.payee.clone()),
                notes: Some(tx.notes.clone()),
                external_id: Some(tx.external_id.clone()),
                manual_account_id: Some(tx.manual_account_id),
                status: Some(tx.status),
                tag_ids: tx.tag_ids.clone(),
                category_id: tx.category_id,
                custom_metadata: tx.custom_metadata.clone(),
            })
            .collect();
        self.0.insert_transactions(&lib_txs).await
    }

    pub async fn update_transactions(&self, txs: &[schema::UpdateObject]) -> anyhow::Result<()> {
        let lib_txs: Vec<lunch_money::schema::UpdateObject<LunchMoneyTxMetadata, ExternalId>> = txs
            .iter()
            .map(|tx| lunch_money::schema::UpdateObject {
                id: tx.id,
                date: Some(tx.date),
                amount: Some(tx.amount),
                currency: Some(tx.currency.clone()),
                payee: Some(tx.payee.clone()),
                notes: Some(tx.notes.clone()),
                custom_metadata: tx.custom_metadata.clone(),
                additional_tag_ids: tx.additional_tag_ids.clone(),
                external_id: tx.external_id.clone(),
            })
            .collect();
        self.0.update_transactions(&lib_txs).await
    }

    pub async fn delete_transactions(&self, ids: &[u64]) -> anyhow::Result<()> {
        self.0.delete_transactions(ids).await
    }

    pub async fn update_manual_account(&self, id: u64, balance: Decimal) -> anyhow::Result<()> {
        self.0.update_manual_account(id, balance).await
    }
}

pub mod schema {
    use super::*;

    pub type Transaction = lunch_money::schema::Transaction<MaybeLunchMoneyTxMetadata, ExternalId>;
    pub type InsertTransactionsResponse =
        lunch_money::schema::InsertTransactionsResponse<MaybeLunchMoneyTxMetadata, ExternalId>;

    pub use crate::metadata::LunchMoneyTxMetadata;
    pub use crate::metadata::MaybeLunchMoneyTxMetadata;

    pub use lunch_money::schema::AccountStatus;
    pub use lunch_money::schema::AccountType;
    pub use lunch_money::schema::Category;
    pub use lunch_money::schema::ManualAccount;
    pub use lunch_money::schema::Tag;
    pub use lunch_money::schema::TransactionStatus;

    #[derive(serde::Serialize, Clone, Debug)]
    pub struct InsertObject {
        pub date: jiff::civil::Date,
        pub amount: Decimal,
        pub currency: Currency,
        pub payee: String,
        pub notes: String,
        pub external_id: ExternalId,
        pub manual_account_id: u64,
        pub status: TransactionStatus,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub tag_ids: Option<Vec<u64>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub category_id: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub custom_metadata: Option<LunchMoneyTxMetadata>,
    }

    #[derive(serde::Serialize, Clone, Debug)]
    pub struct UpdateObject {
        pub id: u64,
        pub date: jiff::civil::Date,
        pub amount: Decimal,
        pub currency: Currency,
        pub payee: String,
        pub notes: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub custom_metadata: Option<LunchMoneyTxMetadata>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub additional_tag_ids: Option<Vec<u64>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[expect(clippy::option_option)]
        pub external_id: Option<Option<ExternalId>>,
    }
}
