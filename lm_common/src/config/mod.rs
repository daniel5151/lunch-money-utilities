//! The unified `lm_utils.toml` config model and its `toml_edit`-backed loader.
//!
//! Every tool previously read its own `lm_<tool>.toml` with a private
//! `[lunch_money]` table, duplicating the one Lunch Money API key across three
//! files. This module replaces that with a single document holding a shared
//! `[common]` table plus one section per tool:
//!
//! ```toml
//! [common]
//! lm_api_key = "..."          # the single shared Lunch Money key
//! # retry = { max_attempts = 5, initial_delay = "2s" }   # optional
//!
//! [payslip]
//! # ...
//!
//! [splitwise]
//! # ...
//!
//! [venmo]
//! # ...
//! ```
//!
//! The document is parsed with [`toml_edit`] (preserving comments and ordering
//! so `init` can rewrite it in place — see [`editor`]). Each tool extracts and
//! deserializes only its own section via [`deserialize_section`]; the shared
//! [`CommonConfig`] is read via [`common_section`]. The loader itself knows
//! nothing about any specific tool's section shape.

pub mod editor;
pub mod loader;

pub use loader::DEFAULT_CONFIG_FILENAME;
pub use loader::common_section;
pub use loader::deserialize_section;
pub use loader::load_document;
pub use loader::optional_section;

use crate::lm_client::RetryConfig;

/// The shared `[common]` config table.
///
/// Holds the single Lunch Money API key (previously duplicated into every
/// tool's `[lunch_money]` table) and the configurable 429 retry policy. The key
/// is optional because the payslip importer can run keyless under `--dry-run`.
#[derive(Debug, Clone, Default, serde::Deserialize, serde::Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct CommonConfig {
    /// The single shared Lunch Money developer API key. `None` when omitted
    /// (only valid for tools/modes that do not contact Lunch Money, e.g. the
    /// payslip importer under `--dry-run`).
    pub lm_api_key: Option<String>,
    /// The 429 (Too Many Requests) retry policy applied to the Lunch Money
    /// client. Defaults to the behavior every tool previously hardcoded.
    pub retry: RetryConfig,
}
