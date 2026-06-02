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
            use crate::STYLE_ERROR;
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

    #[derive(Deserialize)]
    pub struct GroupResponse {
        pub groups: Vec<Group>,
    }

    #[derive(Deserialize, Debug)]
    pub struct Group {
        pub id: u64,
        pub name: String,
        pub updated_at: jiff::Timestamp,
        pub members: Option<Vec<GroupMember>>,
    }

    #[derive(Deserialize, Debug)]
    pub struct GroupMember {
        pub id: u64,
        pub balance: Vec<Balance>,
    }

    #[derive(Deserialize, Debug)]
    pub struct Balance {
        pub currency_code: String,
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
        pub currency_code: String,
        pub deleted_at: Option<jiff::Timestamp>,
        pub users: Vec<ExpenseUser>,
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
}
