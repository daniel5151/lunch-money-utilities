use crate::style::*;

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
    ) -> T {
        let url = format!("https://secure.splitwise.com/api/v3.0/{}", endpoint);
        let res = self
            .http
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .query(query)
            .send()
            .await
            .expect("Splitwise HTTP call failed");

        if !res.status().is_success() {
            anstream::eprintln!(
                "\n{STYLE_ERROR}❌ Splitwise request failed:{STYLE_ERROR:#} {}\n",
                res.status()
            );
            std::process::exit(1);
        }
        res.json().await.expect("Failed parsing Splitwise JSON")
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
