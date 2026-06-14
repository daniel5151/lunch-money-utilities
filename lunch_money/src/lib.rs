//! Client library for the Lunch Money developer API.
//!
//! This crate provides a client and data models for interacting with
//! the Lunch Money budget tracking API (v2).

#![warn(missing_docs)]
#![forbid(unsafe_code)]

use anyhow::Context;
use schema::ManualAccountId;
use schema::TagId;
use schema::TransactionId;

pub mod schema;

/// A client for the Lunch Money API.
///
/// Holds the HTTP client and developer API key used to make authenticated requests.
#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    api_key: String,
}

impl Client {
    /// Creates a new `Client` with the given HTTP client and API key.
    pub fn new(http: reqwest::Client, api_key: String) -> Self {
        Self { http, api_key }
    }

    async fn fetch<T: serde::de::DeserializeOwned, Q: serde::Serialize + ?Sized>(
        &self,
        endpoint: &str,
        query: &Q,
    ) -> anyhow::Result<T> {
        let url = format!("https://api.lunchmoney.dev/v2/{}", endpoint);
        let res = self
            .http
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .query(query)
            .send()
            .await
            .context("Lunch Money HTTP call failed")?;

        if !res.status().is_success() {
            let status = res.status();
            let body = res.text().await.unwrap_or_default();
            anyhow::bail!("Lunch Money request failed ({}): {}", status, body.trim());
        }
        res.json().await.context("Failed parsing Lunch Money JSON")
    }

    async fn exec<P: serde::Serialize>(
        &self,
        method: reqwest::Method,
        endpoint: &str,
        payload: &P,
    ) -> anyhow::Result<()> {
        let url = format!("https://api.lunchmoney.dev/v2/{}", endpoint);
        let res = self
            .http
            .request(method, &url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(payload)
            .send()
            .await
            .context("Lunch Money HTTP call failed")?;

        if !res.status().is_success() {
            let status = res.status();
            let body = res.text().await.unwrap_or_default();
            anyhow::bail!("Lunch Money request failed ({}): {}", status, body.trim());
        }
        Ok(())
    }

    async fn exec_with_response<T: serde::de::DeserializeOwned, P: serde::Serialize>(
        &self,
        method: reqwest::Method,
        endpoint: &str,
        payload: &P,
    ) -> anyhow::Result<T> {
        let url = format!("https://api.lunchmoney.dev/v2/{}", endpoint);
        let res = self
            .http
            .request(method, &url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(payload)
            .send()
            .await
            .context("Lunch Money HTTP call failed")?;

        if !res.status().is_success() {
            let status = res.status();
            let body = res.text().await.unwrap_or_default();
            anyhow::bail!("Lunch Money request failed ({}): {}", status, body.trim());
        }
        res.json().await.context("Failed parsing Lunch Money JSON")
    }

    /// Fetches all manual accounts associated with the user's account.
    pub async fn fetch_manual_accounts(&self) -> anyhow::Result<Vec<schema::ManualAccount>> {
        let res: schema::ManualAccountsResponse = self
            .fetch("manual_accounts", &[] as &[(&str, &str)])
            .await?;
        Ok(res.manual_accounts)
    }

    /// Fetches transactions matching the specified query parameters.
    pub async fn fetch_transactions<T, E>(
        &self,
        query: &TransactionQuery,
    ) -> anyhow::Result<Vec<schema::Transaction<T, E>>>
    where
        T: serde::de::DeserializeOwned,
        E: serde::de::DeserializeOwned,
    {
        let res: schema::TransactionsResponse<T, E> = self.fetch("transactions", query).await?;
        Ok(res.transactions)
    }

    /// Fetches a single transaction by its unique ID. Returns `None` if the transaction is not found.
    pub async fn fetch_transaction_by_id<T, E>(
        &self,
        id: TransactionId,
    ) -> anyhow::Result<Option<schema::Transaction<T, E>>>
    where
        T: serde::de::DeserializeOwned,
        E: serde::de::DeserializeOwned,
    {
        let url = format!("https://api.lunchmoney.dev/v2/transactions/{}", id);
        let res = self
            .http
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await
            .context("Lunch Money HTTP call failed")?;

        if res.status() == reqwest::StatusCode::NOT_FOUND {
            let body = res.text().await.unwrap_or_default();
            if body.contains("There is no transaction with the id") {
                return Ok(None);
            } else {
                anyhow::bail!(
                    "Lunch Money request failed (404 Not Found): {}",
                    body.trim()
                );
            }
        }

        if !res.status().is_success() {
            let status = res.status();
            let body = res.text().await.unwrap_or_default();
            anyhow::bail!("Lunch Money request failed ({}): {}", status, body.trim());
        }

        let parsed = res
            .json::<schema::Transaction<T, E>>()
            .await
            .context("Failed parsing Lunch Money JSON")?;
        Ok(Some(parsed))
    }

    /// Fetches all categories for the user, optionally with formatting (e.g. flat or nested).
    pub async fn fetch_categories(
        &self,
        format: Option<&str>,
    ) -> anyhow::Result<Vec<schema::Category>> {
        let q = format.map(|f| vec![("format", f)]).unwrap_or_default();
        let res: schema::CategoriesResponse = self.fetch("categories", &q).await?;
        Ok(res.categories)
    }

    /// Fetches all tags associated with the user's account.
    pub async fn fetch_tags(&self) -> anyhow::Result<Vec<schema::Tag>> {
        let res: schema::TagsResponse = self.fetch("tags", &[] as &[(&str, &str)]).await?;
        Ok(res.tags)
    }

    /// Creates a new tag with the specified name and optional description.
    pub async fn create_tag(
        &self,
        name: &str,
        description: Option<&str>,
    ) -> anyhow::Result<schema::Tag> {
        self.exec_with_response(
            reqwest::Method::POST,
            "tags",
            &schema::CreateTagPayload {
                name: name.to_string(),
                description: description.map(|s| s.to_string()),
            },
        )
        .await
    }

    /// Inserts a list of new transactions.
    pub async fn insert_transactions<T, E, U, V>(
        &self,
        txs: &[schema::InsertObject<T, E>],
    ) -> anyhow::Result<schema::InsertTransactionsResponse<U, V>>
    where
        T: serde::Serialize + Clone,
        E: serde::Serialize + Clone,
        U: serde::de::DeserializeOwned,
        V: serde::de::DeserializeOwned,
    {
        self.exec_with_response(
            reqwest::Method::POST,
            "transactions",
            &schema::InsertPayload {
                transactions: txs.to_vec(),
            },
        )
        .await
    }

    /// Updates a list of existing transactions.
    pub async fn update_transactions<T, E>(
        &self,
        txs: &[schema::UpdateObject<T, E>],
    ) -> anyhow::Result<()>
    where
        T: serde::Serialize + Clone,
        E: serde::Serialize + Clone,
    {
        self.exec(
            reqwest::Method::PUT,
            "transactions",
            &schema::UpdatePayload {
                transactions: txs.to_vec(),
            },
        )
        .await
    }

    /// Deletes transactions by their IDs.
    pub async fn delete_transactions(&self, ids: &[TransactionId]) -> anyhow::Result<()> {
        self.exec(
            reqwest::Method::DELETE,
            "transactions",
            &schema::DeletePayload { ids: ids.to_vec() },
        )
        .await
    }

    /// Updates the balance of a manual account.
    pub async fn update_manual_account(
        &self,
        id: ManualAccountId,
        balance: rust_decimal::Decimal,
    ) -> anyhow::Result<()> {
        self.exec(
            reqwest::Method::PUT,
            &format!("manual_accounts/{}", id),
            &schema::UpdateManualAccountObject { balance },
        )
        .await
    }
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
