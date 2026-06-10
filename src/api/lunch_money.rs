use anyhow::Context;

pub struct Client {
    http: reqwest::Client,
    api_key: String,
}

impl Client {
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

    pub async fn fetch_manual_accounts(&self) -> anyhow::Result<Vec<schema::ManualAccount>> {
        let res: schema::ManualAccountsResponse = self
            .fetch("manual_accounts", &[] as &[(&str, &str)])
            .await?;
        Ok(res.manual_accounts)
    }

    pub async fn fetch_transactions(
        &self,
        query: &TransactionQuery,
    ) -> anyhow::Result<Vec<schema::Transaction>> {
        let res: schema::TransactionsResponse = self.fetch("transactions", query).await?;
        Ok(res.transactions)
    }

    pub async fn fetch_categories(
        &self,
        format: Option<&str>,
    ) -> anyhow::Result<Vec<schema::Category>> {
        let q = format.map(|f| vec![("format", f)]).unwrap_or_default();
        let res: schema::CategoriesResponse = self.fetch("categories", &q).await?;
        Ok(res.categories)
    }

    pub async fn fetch_tags(&self) -> anyhow::Result<Vec<schema::Tag>> {
        let res: schema::TagsResponse = self.fetch("tags", &[] as &[(&str, &str)]).await?;
        Ok(res.tags)
    }

    pub async fn create_tag(&self, name: &str) -> anyhow::Result<schema::Tag> {
        self.exec_with_response(
            reqwest::Method::POST,
            "tags",
            &schema::CreateTagPayload {
                name: name.to_string(),
            },
        )
        .await
    }

    pub async fn insert_transactions(
        &self,
        txs: &[schema::InsertObject],
    ) -> anyhow::Result<schema::InsertTransactionsResponse> {
        self.exec_with_response(
            reqwest::Method::POST,
            "transactions",
            &schema::InsertPayload {
                transactions: txs.to_vec(),
            },
        )
        .await
    }

    pub async fn update_transactions(&self, txs: &[schema::UpdateObject]) -> anyhow::Result<()> {
        self.exec(
            reqwest::Method::PUT,
            "transactions",
            &schema::UpdatePayload {
                transactions: txs.to_vec(),
            },
        )
        .await
    }

    pub async fn delete_transactions(&self, ids: &[u64]) -> anyhow::Result<()> {
        self.exec(
            reqwest::Method::DELETE,
            "transactions",
            &schema::DeletePayload { ids: ids.to_vec() },
        )
        .await
    }

    pub async fn update_manual_account(
        &self,
        id: u64,
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

pub mod schema {
    pub use crate::metadata::LunchMoneyTxMetadata;
    pub use crate::metadata::MaybeLunchMoneyTxMetadata;
    use rust_decimal::Decimal;
    use serde::Deserialize;
    use serde::Serialize;

    #[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
    #[serde(rename_all = "lowercase")]
    pub enum TransactionStatus {
        Reviewed,
        Unreviewed,
        #[serde(rename = "delete_pending")]
        DeletePending,
    }

    #[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
    #[serde(rename_all = "lowercase")]
    pub enum AccountStatus {
        Active,
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

    #[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
    #[serde(rename_all = "lowercase")]
    pub enum AccountType {
        Cash,
        Credit,
        Cryptocurrency,
        #[serde(rename = "employee compensation")]
        EmployeeCompensation,
        Investment,
        Loan,
        #[serde(rename = "other liability")]
        OtherLiability,
        #[serde(rename = "other asset")]
        OtherAsset,
        #[serde(rename = "real estate")]
        RealEstate,
        Vehicle,
    }

    #[derive(Deserialize)]
    pub struct TransactionsResponse {
        pub transactions: Vec<Transaction>,
    }

    #[derive(Deserialize, Clone, Debug)]
    pub struct Transaction {
        pub id: u64,
        pub date: jiff::civil::Date,
        pub amount: Decimal,
        pub currency: crate::api::Currency,
        pub payee: String,
        pub notes: Option<String>,
        pub external_id: Option<crate::api::ExternalId>,
        pub manual_account_id: Option<u64>,
        pub is_split_parent: Option<bool>,
        pub group_parent_id: Option<u64>,
        pub status: TransactionStatus,
        pub category_id: Option<u64>,
        pub custom_metadata: Option<MaybeLunchMoneyTxMetadata>,
    }

    #[derive(Serialize, Debug)]
    pub struct InsertPayload {
        pub transactions: Vec<InsertObject>,
    }

    #[derive(Serialize, Clone, Debug)]
    pub struct InsertObject {
        pub date: jiff::civil::Date,
        pub amount: Decimal,
        pub currency: crate::api::Currency,
        pub payee: String,
        pub notes: String,
        pub external_id: crate::api::ExternalId,
        pub manual_account_id: u64,
        pub status: TransactionStatus,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub tag_ids: Option<Vec<u64>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub category_id: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub custom_metadata: Option<LunchMoneyTxMetadata>,
    }

    #[derive(Deserialize, Debug)]
    pub struct TagsResponse {
        pub tags: Vec<Tag>,
    }

    #[derive(Deserialize, Clone, Debug)]
    pub struct Tag {
        pub id: u64,
        pub name: String,
        pub archived: bool,
    }

    #[derive(Serialize, Debug)]
    pub struct CreateTagPayload {
        pub name: String,
    }

    #[derive(Serialize, Debug)]
    pub struct UpdatePayload {
        pub transactions: Vec<UpdateObject>,
    }

    #[derive(Serialize, Clone, Debug)]
    pub struct UpdateObject {
        pub id: u64,
        pub date: jiff::civil::Date,
        pub amount: Decimal,
        pub currency: crate::api::Currency,
        pub payee: String,
        pub notes: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub custom_metadata: Option<LunchMoneyTxMetadata>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub additional_tag_ids: Option<Vec<u64>>,
    }

    #[derive(Deserialize, Debug)]
    pub struct InsertTransactionsResponse {
        pub transactions: Vec<Transaction>,
        pub skipped_duplicates: Vec<SkippedExistingExternalIdObject>,
    }

    #[derive(Deserialize, Clone, Debug)]
    pub struct SkippedExistingExternalIdObject {
        pub reason: String,
        pub request_transactions_index: usize,
        pub existing_transaction_id: u64,
    }

    #[derive(Serialize, Debug)]
    pub struct DeletePayload {
        pub ids: Vec<u64>,
    }

    #[derive(Serialize, Clone, Debug)]
    pub struct UpdateManualAccountObject {
        pub balance: Decimal,
    }

    #[derive(Deserialize, Debug)]
    pub struct CategoriesResponse {
        pub categories: Vec<Category>,
    }

    #[derive(Deserialize, Clone, Debug)]
    pub struct Category {
        pub id: u64,
        pub name: String,
        pub is_group: bool,
        pub group_id: Option<u64>,
        pub archived: bool,
        pub children: Option<Vec<ChildCategory>>,
    }

    #[derive(Deserialize, Clone, Debug)]
    pub struct ChildCategory {
        pub id: u64,
        pub name: String,
        pub group_id: Option<u64>,
        pub archived: bool,
    }

    #[derive(serde::Deserialize, Clone, Debug)]
    pub struct ManualAccount {
        pub id: u64,
        pub name: String,
        pub display_name: Option<String>,
        #[serde(rename = "type")]
        pub account_type: AccountType,
        #[serde(with = "rust_decimal::serde::str")]
        pub balance: Decimal,
        pub currency: crate::api::Currency,
        pub status: AccountStatus,
    }

    #[derive(serde::Deserialize, Debug)]
    pub struct ManualAccountsResponse {
        pub manual_accounts: Vec<ManualAccount>,
    }
}
