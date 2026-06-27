//! User profile schemas.

/// JSON schemas for user profiles.
pub mod schemas {
    use serde::Deserialize;

    use crate::core::Currency;
    use crate::core::UserId;

    /// Represents a Lunch Money user profile.
    #[derive(Deserialize, Debug, Clone)]
    pub struct User {
        /// The user's name.
        pub name: String,
        /// The user's email address.
        pub email: String,
        /// Unique identifier for the user.
        pub id: UserId,
        /// Unique identifier for the linked budgeting account.
        pub account_id: u64,
        /// Name of the linked budgeting account.
        pub budget_name: String,
        /// Primary currency set in the user's settings.
        pub primary_currency: Currency,
        /// Label assigned to the API key being used, or `None` if no label is set.
        pub api_key_label: Option<String>,
    }
}
