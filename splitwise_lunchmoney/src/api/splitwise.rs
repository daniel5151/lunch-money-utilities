use anyhow::Context;

pub struct Client {
    http: reqwest::Client,
    api_key: String,
}

#[derive(Debug, Clone)]
pub struct Expense {
    pub raw: serde_json::Value,
    pub parsed: schema::Expense,
}

impl<'de> serde::Deserialize<'de> for Expense {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = serde_json::Value::deserialize(deserializer)?;
        let parsed = schema::Expense::deserialize(raw.clone()).map_err(serde::de::Error::custom)?;
        Ok(Self { raw, parsed })
    }
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

    pub async fn fetch_current_user(&self) -> anyhow::Result<schema::User> {
        let res: schema::CurrentUserResponse = self
            .fetch("get_current_user", &[] as &[(&str, &str)])
            .await?;
        Ok(res.user)
    }

    pub async fn fetch_categories(&self) -> anyhow::Result<Vec<schema::ParentCategory>> {
        let res: schema::CategoriesResponse =
            self.fetch("get_categories", &[] as &[(&str, &str)]).await?;
        Ok(res.categories)
    }

    pub async fn fetch_groups(&self) -> anyhow::Result<Vec<schema::Group>> {
        let res: schema::GroupResponse = self.fetch("get_groups", &[] as &[(&str, &str)]).await?;
        Ok(res.groups)
    }

    pub async fn fetch_expenses(&self, query: &ExpensesQuery) -> anyhow::Result<Vec<Expense>> {
        let res: serde_json::Value = self.fetch("get_expenses", query).await?;
        let raw_expenses = match res.get("expenses") {
            Some(serde_json::Value::Array(arr)) => arr,
            _ => anyhow::bail!("Expected 'expenses' key to contain an array in Splitwise response"),
        };

        let mut expenses = Vec::with_capacity(raw_expenses.len());
        for val in raw_expenses {
            let parsed: schema::Expense =
                serde_json::from_value(val.clone()).context("Failed to parse Splitwise expense")?;
            expenses.push(Expense {
                raw: val.clone(),
                parsed,
            });
        }
        Ok(expenses)
    }

    pub async fn fetch_expense(&self, id: u64) -> anyhow::Result<Expense> {
        let res: serde_json::Value = self
            .fetch(&format!("get_expense/{}", id), &[] as &[(&str, &str)])
            .await?;
        let val = res
            .get("expense")
            .context("Expected 'expense' key in Splitwise response")?;
        let parsed: schema::Expense =
            serde_json::from_value(val.clone()).context("Failed to parse Splitwise expense")?;
        Ok(Expense {
            raw: val.clone(),
            parsed,
        })
    }

    pub async fn fetch_friends(&self) -> anyhow::Result<Vec<schema::Friend>> {
        let res: schema::FriendsResponse =
            self.fetch("get_friends", &[] as &[(&str, &str)]).await?;
        Ok(res.friends)
    }

    pub async fn fetch_notifications(
        &self,
        query: &NotificationsQuery,
    ) -> anyhow::Result<Vec<schema::Notification>> {
        let res: schema::NotificationsResponse = self.fetch("get_notifications", query).await?;
        Ok(res.notifications)
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_after: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_before: Option<String>,
}

#[derive(serde::Serialize, Debug, Clone, Default)]
pub struct NotificationsQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_after: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

pub mod schema {
    use rust_decimal::Decimal;
    use serde::Deserialize;

    #[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
    #[serde(rename_all = "lowercase")]
    pub enum RepeatInterval {
        Never,
        Weekly,
        Fortnightly,
        Monthly,
        Yearly,
    }

    #[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
    pub enum CommentType {
        System,
        User,
    }

    #[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
    pub enum CommentRelationType {
        ExpenseComment,
    }

    #[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
    #[serde(rename_all = "lowercase")]
    pub enum RegistrationStatus {
        Confirmed,
        Dummy,
        Invited,
    }

    #[derive(Deserialize, Debug, Clone)]
    pub struct Repayment {
        pub from: u64,
        pub to: u64,
        #[serde(with = "rust_decimal::serde::str")]
        pub amount: Decimal,
    }

    #[derive(Deserialize, Debug, Clone)]
    pub struct Receipt {
        pub large: Option<String>,
        pub original: Option<String>,
    }

    #[derive(Deserialize, Debug, Clone)]
    pub struct SlimIcon {
        pub small: Option<String>,
        pub large: Option<String>,
    }

    #[derive(Deserialize, Debug, Clone)]
    pub struct SquareIcon {
        pub large: Option<String>,
        pub xlarge: Option<String>,
    }

    #[derive(Deserialize, Debug, Clone)]
    pub struct IconTypes {
        pub slim: Option<SlimIcon>,
        pub square: Option<SquareIcon>,
    }

    #[derive(Deserialize, Debug, Clone)]
    pub struct UserPicture {
        pub medium: Option<String>,
    }

    #[derive(Deserialize, Debug, Clone)]
    pub struct UserPictureDetailed {
        pub small: Option<String>,
        pub medium: Option<String>,
        pub large: Option<String>,
    }

    #[derive(Deserialize, Debug, Clone)]
    pub struct Comment {
        pub id: u64,
        pub content: String,
        pub comment_type: CommentType,
        pub relation_type: CommentRelationType,
        pub relation_id: u64,
        pub created_at: jiff::Timestamp,
        pub deleted_at: Option<jiff::Timestamp>,
        pub user: Option<UserDetails>,
    }

    #[derive(Deserialize, Debug, Clone)]
    pub struct FriendsResponse {
        pub friends: Vec<Friend>,
    }

    #[derive(Deserialize, Debug, Clone)]
    pub struct Friend {
        pub id: u64,
        pub first_name: String,
        pub last_name: Option<String>,
        pub balance: Vec<Balance>,
    }

    #[derive(Deserialize, Debug, Clone)]
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

    #[derive(Deserialize, Debug, Clone)]
    pub struct Expense {
        pub id: u64,
        pub group_id: Option<u64>,
        pub friendship_id: Option<u64>,
        pub expense_bundle_id: Option<u64>,
        pub description: String,
        #[serde(default)]
        pub repeats: bool,
        pub repeat_interval: Option<RepeatInterval>,
        pub email_reminder: Option<bool>,
        pub email_reminder_in_advance: Option<i32>,
        pub next_repeat: Option<String>,
        pub details: Option<String>,
        pub comments_count: Option<u32>,
        #[serde(default)]
        pub payment: bool,
        pub transaction_confirmed: Option<bool>,
        #[serde(default, with = "rust_decimal::serde::str_option")]
        pub cost: Option<Decimal>,
        pub currency_code: crate::api::Currency,
        #[serde(default)]
        pub repayments: Vec<Repayment>,
        pub date: jiff::Timestamp,
        pub created_at: Option<jiff::Timestamp>,
        pub created_by: Option<User>,
        pub updated_at: Option<jiff::Timestamp>,
        pub updated_by: Option<User>,
        pub deleted_at: Option<jiff::Timestamp>,
        pub deleted_by: Option<User>,
        pub category: Option<Category>,
        pub category_id: Option<u32>,
        pub receipt: Option<Receipt>,
        pub users: Vec<ExpenseUser>,
        pub comments: Option<Vec<Comment>>,
    }

    #[derive(Deserialize, Debug, Clone)]
    pub struct ExpenseUser {
        pub user_id: u64,
        #[serde(with = "rust_decimal::serde::str")]
        pub net_balance: Decimal,
        #[serde(default, with = "rust_decimal::serde::str_option")]
        pub paid_share: Option<Decimal>,
        #[serde(default, with = "rust_decimal::serde::str_option")]
        pub owed_share: Option<Decimal>,
        pub user: Option<UserDetails>,
    }

    #[derive(Deserialize, Debug, Clone)]
    pub struct UserDetails {
        pub id: Option<u64>,
        pub first_name: Option<String>,
        pub last_name: Option<String>,
        pub picture: Option<UserPicture>,
    }

    #[derive(Deserialize, Debug, Clone)]
    pub struct CategoriesResponse {
        pub categories: Vec<ParentCategory>,
    }

    #[derive(Deserialize, Debug, Clone)]
    pub struct Category {
        pub id: u32,
        pub name: String,
        pub icon: Option<String>,
        pub icon_types: Option<IconTypes>,
    }

    #[derive(Deserialize, Debug, Clone)]
    pub struct ParentCategory {
        pub id: u32,
        pub name: String,
        pub subcategories: Vec<Category>,
    }

    #[derive(Deserialize, Debug, Clone)]
    pub struct CurrentUserResponse {
        pub user: User,
    }

    #[derive(Deserialize, Debug, Clone)]
    pub struct User {
        pub id: u64,
        pub first_name: String,
        pub last_name: Option<String>,
        pub email: Option<String>,
        pub registration_status: Option<RegistrationStatus>,
        pub picture: Option<UserPictureDetailed>,
        pub custom_picture: Option<bool>,
    }

    #[derive(Deserialize, Debug, Clone)]
    pub struct NotificationSource {
        pub r#type: String,
        pub id: u64,
        pub url: Option<String>,
    }

    #[derive(Deserialize, Debug, Clone)]
    pub struct Notification {
        pub id: u64,
        pub r#type: u32,
        pub created_at: jiff::Timestamp,
        pub created_by: Option<u64>,
        pub source: Option<NotificationSource>,
        pub image_url: Option<String>,
        pub image_shape: Option<String>,
        pub content: String,
    }

    #[derive(Deserialize, Debug, Clone)]
    pub struct NotificationsResponse {
        pub notifications: Vec<Notification>,
    }
}
