//! Data structures representing schemas returned by or sent to the Lunch Money API.

#![warn(missing_docs)]
#![forbid(unsafe_code)]

/// Sample `reqwest`-based Client implementation for interacting with the Lunch
/// Money developer API.
#[cfg(feature = "client")]
pub mod client;

/// Category schema models and responses.
pub mod categories;
/// Core shared types like Currency and IDs.
pub mod core;
/// Manual account schema models, types, and responses.
pub mod manual_accounts;
/// Tag schema models and payloads.
pub mod tags;
/// Transaction schema models, filters, and payloads.
pub mod transactions;
