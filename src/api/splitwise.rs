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
        let url = format!("https://secure.splitwise.com/api/v3.0/{}", endpoint);
        let res = self
            .http
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .query(query)
            .send()
            .await
            .context("Splitwise HTTP call failed")?;

        if !res.status().is_success() {
            anyhow::bail!("Splitwise request failed: {}", res.status());
        }
        res.json().await.context("Failed parsing Splitwise JSON")
    }
}

#[derive(serde::Serialize, Debug, Clone, Default)]
pub struct ExpensesQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dated_after: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dated_before: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

pub trait SplitwiseService: Send + Sync {
    async fn fetch_categories(&self) -> anyhow::Result<Vec<schema::ParentCategory>>;
    async fn fetch_groups(&self) -> anyhow::Result<Vec<schema::Group>>;
    async fn fetch_expenses(&self, query: &ExpensesQuery) -> anyhow::Result<Vec<schema::Expense>>;
    async fn fetch_friends(&self) -> anyhow::Result<Vec<schema::Friend>>;
}

impl SplitwiseService for Client {
    async fn fetch_categories(&self) -> anyhow::Result<Vec<schema::ParentCategory>> {
        let res: schema::CategoriesResponse =
            self.fetch("get_categories", &[] as &[(&str, &str)]).await?;
        Ok(res.categories)
    }

    async fn fetch_groups(&self) -> anyhow::Result<Vec<schema::Group>> {
        let res: schema::GroupResponse = self.fetch("get_groups", &[] as &[(&str, &str)]).await?;
        Ok(res.groups)
    }

    async fn fetch_expenses(&self, query: &ExpensesQuery) -> anyhow::Result<Vec<schema::Expense>> {
        let res: schema::ExpensesResponse = self.fetch("get_expenses", query).await?;
        Ok(res.expenses)
    }

    async fn fetch_friends(&self) -> anyhow::Result<Vec<schema::Friend>> {
        let res: schema::FriendsResponse =
            self.fetch("get_friends", &[] as &[(&str, &str)]).await?;
        Ok(res.friends)
    }
}

pub mod schema {
    use rust_decimal::Decimal;
    use serde::Deserialize;

    #[derive(Deserialize, Debug)]
    pub struct FriendsResponse {
        pub friends: Vec<Friend>,
    }

    #[derive(Deserialize, Debug)]
    pub struct Friend {
        #[expect(dead_code)]
        pub id: u64,
        #[expect(dead_code)]
        pub first_name: String,
        #[expect(dead_code)]
        pub last_name: Option<String>,
        pub balance: Vec<Balance>,
    }

    #[derive(Deserialize)]
    pub struct GroupResponse {
        pub groups: Vec<Group>,
    }

    #[derive(Deserialize, Debug, Clone)]
    pub struct Group {
        pub id: u64,
        pub name: String,
        pub updated_at: jiff::Timestamp,
        pub members: Option<Vec<GroupMember>>,
    }

    #[derive(Deserialize, Debug, Clone)]
    pub struct GroupMember {
        pub id: u64,
        pub balance: Vec<Balance>,
    }

    #[derive(Deserialize, Debug, Clone)]
    pub struct Balance {
        pub currency_code: crate::api::Currency,
        #[serde(with = "rust_decimal::serde::str")]
        pub amount: Decimal,
    }

    #[derive(Deserialize)]
    pub struct ExpensesResponse {
        pub expenses: Vec<Expense>,
    }

    #[derive(Deserialize)]
    pub struct Expense {
        pub id: u64,
        pub group_id: Option<u64>,
        pub description: String,
        pub date: jiff::Timestamp,
        pub currency_code: crate::api::Currency,
        pub deleted_at: Option<jiff::Timestamp>,
        pub users: Vec<ExpenseUser>,
        pub category: Option<Category>,
        #[serde(default)]
        pub payment: bool,
    }

    #[derive(Deserialize)]
    pub struct ExpenseUser {
        pub user_id: u64,
        #[serde(with = "rust_decimal::serde::str")]
        pub net_balance: Decimal,
        pub user: Option<UserDetails>,
    }

    #[derive(Deserialize)]
    pub struct UserDetails {
        pub first_name: Option<String>,
        pub last_name: Option<String>,
    }

    #[derive(Deserialize, Debug)]
    pub struct CategoriesResponse {
        pub categories: Vec<ParentCategory>,
    }

    #[derive(Deserialize, Debug, Clone)]
    pub struct Category {
        pub id: u32,
        pub name: String,
    }

    #[derive(Deserialize, Debug)]
    pub struct ParentCategory {
        pub id: u32,
        pub name: String,
        pub subcategories: Vec<Category>,
    }
}
