use crate::categories::schemas::CategoriesResponse;
use crate::categories::schemas::Category;
use crate::core::ids::AttachmentId;
use crate::core::ids::CategoryId;
use crate::core::ids::ManualAccountId;
use crate::core::ids::PlaidAccountId;
use crate::core::ids::RecurringId;
use crate::core::ids::TagId;
use crate::core::ids::TransactionId;
use crate::manual_accounts::schemas::ManualAccount;
use crate::manual_accounts::schemas::ManualAccountsResponse;
use crate::manual_accounts::schemas::UpdateManualAccountObject;
use crate::tags::schemas::CreateTagPayload;
use crate::tags::schemas::Tag;
use crate::tags::schemas::TagsResponse;
use crate::transactions::query_params::TransactionQuery;
use crate::transactions::schemas::DeletePayload;
use crate::transactions::schemas::InsertObject;
use crate::transactions::schemas::InsertPayload;
use crate::transactions::schemas::InsertTransactionsResponse;
use crate::transactions::schemas::Transaction;
use crate::transactions::schemas::TransactionsResponse;
use crate::transactions::schemas::UpdateObject;
use crate::transactions::schemas::UpdatePayload;
use anyhow::Context;

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
    pub async fn fetch_manual_accounts(&self) -> anyhow::Result<Vec<ManualAccount>> {
        let res: ManualAccountsResponse = self
            .fetch("manual_accounts", &[] as &[(&str, &str)])
            .await?;
        Ok(res.manual_accounts)
    }

    /// Fetches transactions matching the specified query parameters.
    ///
    /// Returns the full response including pagination metadata (`has_more`).
    pub async fn fetch_transactions<T, E>(
        &self,
        query: &TransactionQuery,
    ) -> anyhow::Result<TransactionsResponse<T, E>>
    where
        T: serde::de::DeserializeOwned,
        E: serde::de::DeserializeOwned,
    {
        self.fetch("transactions", query).await
    }

    /// Fetches a single transaction by its unique ID. Returns `None` if the transaction is not found.
    pub async fn fetch_transaction_by_id<T, E>(
        &self,
        id: TransactionId,
    ) -> anyhow::Result<Option<Transaction<T, E>>>
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
            .json::<Transaction<T, E>>()
            .await
            .context("Failed parsing Lunch Money JSON")?;
        Ok(Some(parsed))
    }

    /// Fetches all categories for the user, with optional filters.
    pub async fn fetch_categories(
        &self,
        query: &crate::categories::query_params::CategoryQuery,
    ) -> anyhow::Result<Vec<Category>> {
        let res: CategoriesResponse = self.fetch("categories", query).await?;
        Ok(res.categories)
    }

    /// Fetches all tags associated with the user's account.
    pub async fn fetch_tags(&self) -> anyhow::Result<Vec<Tag>> {
        let res: TagsResponse = self.fetch("tags", &[] as &[(&str, &str)]).await?;
        Ok(res.tags)
    }

    /// Creates a new tag with the specified name and optional description.
    pub async fn create_tag(&self, name: &str, description: Option<&str>) -> anyhow::Result<Tag> {
        self.exec_with_response(
            reqwest::Method::POST,
            "tags",
            &CreateTagPayload {
                name: name.to_string(),
                description: description.map(|s| s.to_string()),
                text_color: None,
                background_color: None,
                archived: None,
            },
        )
        .await
    }

    /// Creates a new tag with the specified payload.
    pub async fn create_tag_with_payload(&self, payload: &CreateTagPayload) -> anyhow::Result<Tag> {
        self.exec_with_response(reqwest::Method::POST, "tags", payload)
            .await
    }

    /// Inserts a list of new transactions.
    pub async fn insert_transactions<T, E, U, V>(
        &self,
        txs: &[InsertObject<T, E>],
    ) -> anyhow::Result<InsertTransactionsResponse<U, V>>
    where
        T: serde::Serialize + Clone,
        E: serde::Serialize + Clone,
        U: serde::de::DeserializeOwned,
        V: serde::de::DeserializeOwned,
    {
        self.exec_with_response(
            reqwest::Method::POST,
            "transactions",
            &InsertPayload {
                transactions: txs.to_vec(),
            },
        )
        .await
    }

    /// Updates a list of existing transactions.
    pub async fn update_transactions<T, E>(&self, txs: &[UpdateObject<T, E>]) -> anyhow::Result<()>
    where
        T: serde::Serialize + Clone,
        E: serde::Serialize + Clone,
    {
        self.exec(
            reqwest::Method::PUT,
            "transactions",
            &UpdatePayload {
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
            &DeletePayload { ids: ids.to_vec() },
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
            &UpdateManualAccountObject { balance },
        )
        .await
    }

    async fn exec_empty(&self, method: reqwest::Method, endpoint: &str) -> anyhow::Result<()> {
        let url = format!("https://api.lunchmoney.dev/v2/{}", endpoint);
        let res = self
            .http
            .request(method, &url)
            .header("Authorization", format!("Bearer {}", self.api_key))
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

    // --- User & Summary Endpoints ---

    /// Fetches details about the user associated with the API key.
    pub async fn fetch_user(&self) -> anyhow::Result<crate::users::schemas::User> {
        self.fetch("me", &[] as &[(&str, &str)]).await
    }

    /// Fetches the monthly budget summary for the specified date range and options.
    pub async fn fetch_budget_summary(
        &self,
        query: &crate::budgets::query_params::BudgetSummaryQuery,
    ) -> anyhow::Result<crate::budgets::schemas::BudgetSummary> {
        self.fetch("summary", query).await
    }

    // --- Budgets Endpoints ---

    /// Fetches budget settings for the user's account.
    pub async fn fetch_budget_settings(
        &self,
    ) -> anyhow::Result<crate::budgets::schemas::BudgetSettings> {
        let res: crate::budgets::schemas::BudgetSettingsResponse = self
            .fetch("budgets/settings", &[] as &[(&str, &str)])
            .await?;
        Ok(res.budget_settings)
    }

    /// Creates or updates a budget for a category and period.
    pub async fn upsert_budget(
        &self,
        req: &crate::budgets::schemas::UpsertBudgetRequest,
    ) -> anyhow::Result<crate::budgets::schemas::BudgetUpsertResponse> {
        self.exec_with_response(reqwest::Method::PUT, "budgets", req)
            .await
    }

    /// Deletes the budget for the given category and period.
    pub async fn delete_budget(
        &self,
        category_id: CategoryId,
        start_date: jiff::civil::Date,
    ) -> anyhow::Result<()> {
        let url = "https://api.lunchmoney.dev/v2/budgets";
        let query = vec![
            ("category_id", category_id.to_string()),
            ("start_date", start_date.to_string()),
        ];
        let res = self
            .http
            .delete(url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .query(&query)
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

    // --- Categories Endpoints ---

    /// Creates a new category or category group.
    pub async fn create_category(
        &self,
        payload: &crate::categories::schemas::CreateCategoryPayload,
    ) -> anyhow::Result<Category> {
        self.exec_with_response(reqwest::Method::POST, "categories", payload)
            .await
    }

    /// Fetches a single category by its ID.
    pub async fn fetch_category_by_id(
        &self,
        id: CategoryId,
    ) -> anyhow::Result<Category> {
        self.fetch(&format!("categories/{}", id), &[] as &[(&str, &str)])
            .await
    }

    /// Updates an existing category or category group.
    pub async fn update_category(
        &self,
        id: CategoryId,
        payload: &crate::categories::schemas::UpdateCategoryPayload,
    ) -> anyhow::Result<Category> {
        self.exec_with_response(reqwest::Method::PUT, &format!("categories/{}", id), payload)
            .await
    }

    /// Deletes a category or category group.
    pub async fn delete_category(
        &self,
        id: CategoryId,
        force: Option<bool>,
    ) -> anyhow::Result<crate::categories::schemas::DeleteCategoryResult> {
        let url = format!("https://api.lunchmoney.dev/v2/categories/{}", id);
        let q = force.map(|f| vec![("force", f)]).unwrap_or_default();
        let res = self
            .http
            .delete(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .query(&q)
            .send()
            .await
            .context("Lunch Money HTTP call failed")?;

        if res.status() == reqwest::StatusCode::UNPROCESSABLE_ENTITY {
            let deps = res
                .json::<crate::categories::schemas::DeleteCategoryDependenciesResponse>()
                .await
                .context("Failed parsing delete category dependencies response")?;
            return Ok(crate::categories::schemas::DeleteCategoryResult::Dependencies(deps));
        }

        if !res.status().is_success() {
            let status = res.status();
            let body = res.text().await.unwrap_or_default();
            anyhow::bail!("Lunch Money request failed ({}): {}", status, body.trim());
        }

        Ok(crate::categories::schemas::DeleteCategoryResult::Deleted)
    }

    // --- Manual Accounts Endpoints ---

    /// Creates a manual account.
    pub async fn create_manual_account<E, M>(
        &self,
        payload: &crate::manual_accounts::schemas::CreateManualAccountPayload<E, M>,
    ) -> anyhow::Result<ManualAccount<E, M>>
    where
        E: serde::Serialize + serde::de::DeserializeOwned + Clone,
        M: serde::Serialize + serde::de::DeserializeOwned + Clone,
    {
        self.exec_with_response(reqwest::Method::POST, "manual_accounts", payload)
            .await
    }

    /// Fetches a single manual account by its ID.
    pub async fn fetch_manual_account_by_id<E, M>(
        &self,
        id: ManualAccountId,
    ) -> anyhow::Result<ManualAccount<E, M>>
    where
        E: serde::de::DeserializeOwned,
        M: serde::de::DeserializeOwned,
    {
        self.fetch(&format!("manual_accounts/{}", id), &[] as &[(&str, &str)])
            .await
    }

    /// Deletes a manual account.
    pub async fn delete_manual_account(
        &self,
        id: ManualAccountId,
        delete_items: Option<bool>,
        delete_balance_history: Option<bool>,
    ) -> anyhow::Result<()> {
        let url = format!("https://api.lunchmoney.dev/v2/manual_accounts/{}", id);
        let mut q = Vec::new();
        if let Some(di) = delete_items {
            q.push(("delete_items", di.to_string()));
        }
        if let Some(dbh) = delete_balance_history {
            q.push(("delete_balance_history", dbh.to_string()));
        }
        let res = self
            .http
            .delete(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .query(&q)
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

    /// Updates details of an existing manual account (name, type, etc.).
    pub async fn update_manual_account_details<E, M>(
        &self,
        id: ManualAccountId,
        payload: &crate::manual_accounts::schemas::UpdateManualAccountPayload<E, M>,
    ) -> anyhow::Result<ManualAccount<E, M>>
    where
        E: serde::Serialize + serde::de::DeserializeOwned + Clone,
        M: serde::Serialize + serde::de::DeserializeOwned + Clone,
    {
        self.exec_with_response(
            reqwest::Method::PUT,
            &format!("manual_accounts/{}", id),
            payload,
        )
        .await
    }

    // --- Plaid Accounts Endpoints ---

    /// Fetches all accounts synced via Plaid.
    pub async fn fetch_plaid_accounts(
        &self,
    ) -> anyhow::Result<Vec<crate::plaid_accounts::schemas::PlaidAccount>> {
        let res: crate::plaid_accounts::schemas::PlaidAccountsResponse =
            self.fetch("plaid_accounts", &[] as &[(&str, &str)]).await?;
        Ok(res.plaid_accounts)
    }

    /// Fetches a single Plaid-synced account by its ID.
    pub async fn fetch_plaid_account_by_id(
        &self,
        id: PlaidAccountId,
    ) -> anyhow::Result<crate::plaid_accounts::schemas::PlaidAccount> {
        self.fetch(&format!("plaid_accounts/{}", id), &[] as &[(&str, &str)])
            .await
    }

    /// Triggers a fetch of latest transactions from Plaid.
    pub async fn trigger_plaid_fetch(
        &self,
        query: &crate::plaid_accounts::query_params::TriggerPlaidFetchQuery,
    ) -> anyhow::Result<()> {
        let url = "https://api.lunchmoney.dev/v2/plaid_accounts/fetch";
        let res = self
            .http
            .post(url)
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
        Ok(())
    }

    // --- Transactions Additional Endpoints ---

    /// Updates a single transaction in-place.
    pub async fn update_transaction<T, E>(
        &self,
        id: TransactionId,
        update_balance: Option<bool>,
        tx: &UpdateObject<T, E>,
    ) -> anyhow::Result<Transaction<T, E>>
    where
        T: serde::Serialize + serde::de::DeserializeOwned + Clone,
        E: serde::Serialize + serde::de::DeserializeOwned + Clone,
    {
        let url = format!("https://api.lunchmoney.dev/v2/transactions/{}", id);
        let mut q = Vec::new();
        if let Some(ub) = update_balance {
            q.push(("update_balance", ub));
        }
        let res = self
            .http
            .put(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .query(&q)
            .json(tx)
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

    /// Deletes a single transaction.
    pub async fn delete_transaction(&self, id: TransactionId) -> anyhow::Result<()> {
        self.exec_empty(reqwest::Method::DELETE, &format!("transactions/{}", id))
            .await
    }

    /// Creates a transaction group.
    pub async fn create_transaction_group<T, E>(
        &self,
        payload: &crate::transactions::schemas::CreateTransactionGroupPayload,
    ) -> anyhow::Result<Transaction<T, E>>
    where
        T: serde::de::DeserializeOwned,
        E: serde::de::DeserializeOwned,
    {
        self.exec_with_response(reqwest::Method::POST, "transactions/group", payload)
            .await
    }

    /// Deletes a transaction group (ungroups the transactions).
    pub async fn delete_transaction_group(&self, id: TransactionId) -> anyhow::Result<()> {
        self.exec_empty(
            reqwest::Method::DELETE,
            &format!("transactions/group/{}", id),
        )
        .await
    }

    /// Splits an existing transaction into multiple child transactions.
    pub async fn split_transaction<T, E>(
        &self,
        id: TransactionId,
        payload: &crate::transactions::schemas::SplitTransactionPayload,
    ) -> anyhow::Result<Transaction<T, E>>
    where
        T: serde::de::DeserializeOwned,
        E: serde::de::DeserializeOwned,
    {
        self.exec_with_response(
            reqwest::Method::POST,
            &format!("transactions/split/{}", id),
            payload,
        )
        .await
    }

    /// Unsplits a previously split transaction.
    pub async fn unsplit_transaction(&self, id: TransactionId) -> anyhow::Result<()> {
        self.exec_empty(
            reqwest::Method::DELETE,
            &format!("transactions/split/{}", id),
        )
        .await
    }

    /// Attaches a file to a transaction. File size must not exceed 10MB.
    pub async fn attach_file_to_transaction(
        &self,
        transaction_id: TransactionId,
        file_name: String,
        file_bytes: Vec<u8>,
        mime_type: String,
        notes: Option<&str>,
    ) -> anyhow::Result<crate::transactions::schemas::TransactionAttachment> {
        let url = format!(
            "https://api.lunchmoney.dev/v2/transactions/{}/attachments",
            transaction_id
        );
        let part = reqwest::multipart::Part::bytes(file_bytes)
            .file_name(file_name)
            .mime_str(&mime_type)?;

        let mut form = reqwest::multipart::Form::new().part("file", part);
        if let Some(n) = notes {
            form = form.text("notes", n.to_string());
        }

        let res = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .multipart(form)
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

    /// Fetches a signed download URL for a file attachment.
    pub async fn get_transaction_attachment_url(
        &self,
        file_id: AttachmentId,
    ) -> anyhow::Result<crate::transactions::schemas::AttachmentUrlResponse> {
        self.fetch(
            &format!("transactions/attachments/{}", file_id),
            &[] as &[(&str, &str)],
        )
        .await
    }

    /// Deletes a file attachment from a transaction.
    pub async fn delete_transaction_attachment(&self, file_id: AttachmentId) -> anyhow::Result<()> {
        self.exec_empty(
            reqwest::Method::DELETE,
            &format!("transactions/attachments/{}", file_id),
        )
        .await
    }

    // --- Tags Additional Endpoints ---

    /// Fetches details of a single tag by its ID.
    pub async fn fetch_tag_by_id(&self, id: TagId) -> anyhow::Result<Tag> {
        self.fetch(&format!("tags/{}", id), &[] as &[(&str, &str)])
            .await
    }

    /// Updates details of an existing tag.
    pub async fn update_tag(
        &self,
        id: TagId,
        payload: &crate::tags::schemas::UpdateTagPayload,
    ) -> anyhow::Result<Tag> {
        self.exec_with_response(reqwest::Method::PUT, &format!("tags/{}", id), payload)
            .await
    }

    /// Deletes a tag by its ID.
    pub async fn delete_tag(
        &self,
        id: TagId,
        force: Option<bool>,
    ) -> anyhow::Result<crate::tags::schemas::DeleteTagResult> {
        let url = format!("https://api.lunchmoney.dev/v2/tags/{}", id);
        let q = force.map(|f| vec![("force", f)]).unwrap_or_default();
        let res = self
            .http
            .delete(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .query(&q)
            .send()
            .await
            .context("Lunch Money HTTP call failed")?;

        if res.status() == reqwest::StatusCode::UNPROCESSABLE_ENTITY {
            let deps = res
                .json::<crate::tags::schemas::DeleteTagDependenciesResponse>()
                .await
                .context("Failed parsing delete tag dependencies response")?;
            return Ok(crate::tags::schemas::DeleteTagResult::Dependencies(deps));
        }

        if !res.status().is_success() {
            let status = res.status();
            let body = res.text().await.unwrap_or_default();
            anyhow::bail!("Lunch Money request failed ({}): {}", status, body.trim());
        }

        Ok(crate::tags::schemas::DeleteTagResult::Deleted)
    }

    // --- Recurring Items Endpoints ---

    /// Fetches all recurring items.
    pub async fn fetch_recurring_items(
        &self,
        query: &crate::recurring_items::query_params::RecurringItemsQuery,
    ) -> anyhow::Result<Vec<crate::recurring_items::schemas::RecurringItem>> {
        let res: crate::recurring_items::schemas::RecurringItemsResponse =
            self.fetch("recurring_items", query).await?;
        Ok(res.recurring_items)
    }

    /// Fetches a single recurring item by its ID.
    pub async fn fetch_recurring_item_by_id(
        &self,
        id: RecurringId,
        query: &crate::recurring_items::query_params::RecurringItemQuery,
    ) -> anyhow::Result<crate::recurring_items::schemas::RecurringItem> {
        self.fetch(&format!("recurring_items/{}", id), query).await
    }
}
