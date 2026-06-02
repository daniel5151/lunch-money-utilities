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
        let url = format!("https://api.lunchmoney.dev/v2/{}", endpoint);
        let res = self
            .http
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .query(query)
            .send()
            .await
            .expect("Lunch Money HTTP call failed");

        if !res.status().is_success() {
            use crate::STYLE_ERROR;
            let status = res.status();
            let body = res.text().await.unwrap_or_default();
            anstream::eprintln!(
                "\n{STYLE_ERROR}❌ Lunch Money request failed:{STYLE_ERROR:#} {} - {}\n",
                status,
                body
            );
            std::process::exit(1);
        }
        res.json().await.expect("Failed parsing Lunch Money JSON")
    }

    pub async fn exec<P: serde::Serialize>(
        &self,
        method: reqwest::Method,
        endpoint: &str,
        payload: &P,
    ) {
        let url = format!("https://api.lunchmoney.dev/v2/{}", endpoint);
        let res = self
            .http
            .request(method, &url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(payload)
            .send()
            .await
            .expect("Lunch Money HTTP call failed");

        if !res.status().is_success() {
            use crate::STYLE_ERROR;
            let status = res.status();
            let body = res.text().await.unwrap_or_default();
            anstream::eprintln!(
                "\n{STYLE_ERROR}❌ Lunch Money request failed:{STYLE_ERROR:#} {} - {}\n",
                status,
                body
            );
            std::process::exit(1);
        }
    }
}

pub mod schema {
    use rust_decimal::Decimal;
    use serde::Deserialize;
    use serde::Serialize;

    #[derive(Deserialize)]
    pub struct TransactionsResponse {
        pub transactions: Vec<Transaction>,
    }

    #[derive(Deserialize, Debug)]
    pub struct Transaction {
        pub id: u64,
        pub date: jiff::civil::Date,
        pub amount: Decimal,
        pub currency: String,
        pub payee: String,
        pub notes: Option<String>,
        pub external_id: Option<String>,
        #[allow(dead_code)]
        pub manual_account_id: Option<u64>,
    }

    #[derive(Serialize, Debug)]
    pub struct InsertPayload {
        pub transactions: Vec<InsertObject>,
    }

    #[derive(Serialize, Clone, Debug)]
    pub struct InsertObject {
        pub date: jiff::civil::Date,
        pub amount: Decimal,
        pub currency: String,
        pub payee: String,
        pub notes: String,
        pub external_id: String,
        pub manual_account_id: u64,
        pub status: String,
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
        pub currency: String,
        pub payee: String,
        pub notes: String,
    }

    #[derive(Serialize, Debug)]
    pub struct DeletePayload {
        pub ids: Vec<u64>,
    }
}
