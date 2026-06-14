//! Data structures representing schemas returned by or sent to the Lunch Money API.

#![warn(missing_docs)]
#![forbid(unsafe_code)]

/// Sample `reqwest`-based Client implementation for interacting with the Lunch
/// Money developer API.
#[cfg(feature = "client")]
pub mod client;

/// Budgets and budget summaries.
pub mod budgets;
/// Category schema models and responses.
pub mod categories;
/// Core shared types like Currency and IDs.
pub mod core;
/// Manual account schema models, types, and responses.
pub mod manual_accounts;
/// Synced Plaid account schemas.
pub mod plaid_accounts;
/// Recurring items definitions.
pub mod recurring_items;
/// Tag schema models and payloads.
pub mod tags;
/// Transaction schema models, filters, and payloads.
pub mod transactions;
/// User profile schemas.
pub mod users;
