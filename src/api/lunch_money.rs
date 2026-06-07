use anyhow::Context;

pub struct Client {
    http: reqwest::Client,
    api_key: String,
}

impl Client {
    pub fn new(http: reqwest::Client, api_key: String) -> Self {
        Self { http, api_key }
    }

    pub async fn fetch<T: serde::de::DeserializeOwned, Q: serde::Serialize + ?Sized>(
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

    pub async fn exec<P: serde::Serialize>(
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

    pub async fn exec_with_response<T: serde::de::DeserializeOwned, P: serde::Serialize>(
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
}

pub mod schema {
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

    #[derive(Deserialize, Debug)]
    pub struct Transaction {
        pub id: u64,
        pub date: jiff::civil::Date,
        pub amount: Decimal,
        pub currency: crate::api::Currency,
        pub payee: String,
        pub notes: Option<String>,
        pub external_id: Option<crate::api::ExternalId>,
        #[expect(dead_code)]
        pub manual_account_id: Option<u64>,
        pub is_split_parent: Option<bool>,
        #[expect(dead_code)]
        pub group_parent_id: Option<u64>,
        #[expect(dead_code)]
        pub status: TransactionStatus,
        pub category_id: Option<u64>,
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
        #[expect(dead_code)]
        pub group_id: Option<u64>,
        pub archived: bool,
        pub children: Option<Vec<ChildCategory>>,
    }

    #[derive(Deserialize, Clone, Debug)]
    pub struct ChildCategory {
        pub id: u64,
        pub name: String,
        #[expect(dead_code)]
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
