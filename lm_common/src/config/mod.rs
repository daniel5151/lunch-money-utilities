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
//! so `init` can rewrite it in place — see [`editor`]). Each tool's configuration
//! section is deserialized via [`optional_section`], and the shared
//! [`CommonConfig`] is read via [`common_section`].

pub mod editor;
pub mod loader;

use std::time::Duration;

pub use loader::DEFAULT_CONFIG_FILENAME;
pub use loader::common_section;
pub use loader::optional_section;

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

/// Configurable 429 (Too Many Requests) retry policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetryConfig {
    /// Return the error immediately (fail fast).
    Fail,
    /// Retry the request after a delay.
    Retry(RetryConfigFields),
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self::Retry(RetryConfigFields::default())
    }
}

/// Helper struct for `Retry` variant of `RetryConfig`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct RetryConfigFields {
    /// Maximum number of retry attempts after an initial 429 response.
    pub max_attempts: u32,
    /// Initial backoff delay, parsed/emitted as a humantime string (e.g. `"2s"`).
    #[serde(with = "humantime_duration")]
    pub initial_delay: Duration,
}

impl Default for RetryConfigFields {
    fn default() -> Self {
        Self {
            max_attempts: 5,
            initial_delay: Duration::from_secs(2),
        }
    }
}

impl From<RetryConfig> for lunch_money::client::TooManyRequestsPolicy {
    fn from(value: RetryConfig) -> Self {
        match value {
            RetryConfig::Fail => lunch_money::client::TooManyRequestsPolicy::Fail,
            RetryConfig::Retry(fields) => lunch_money::client::TooManyRequestsPolicy::Retry {
                max_retries: fields.max_attempts,
                initial_delay: fields.initial_delay,
            },
        }
    }
}

impl<'de> serde::Deserialize<'de> for RetryConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct Visitor;

        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = RetryConfig;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a string 'fail' or a retry configuration table")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                if value.eq_ignore_ascii_case("fail") {
                    Ok(RetryConfig::Fail)
                } else {
                    Err(serde::de::Error::custom(format!(
                        "expected 'fail', got '{}'",
                        value
                    )))
                }
            }

            fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::MapAccess<'de>,
            {
                let fields = serde::Deserialize::deserialize(
                    serde::de::value::MapAccessDeserializer::new(map),
                )?;
                Ok(RetryConfig::Retry(fields))
            }
        }

        deserializer.deserialize_any(Visitor)
    }
}

impl serde::Serialize for RetryConfig {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            RetryConfig::Fail => serializer.serialize_str("fail"),
            RetryConfig::Retry(fields) => fields.serialize(serializer),
        }
    }
}

/// serde adapter that (de)serializes a `Duration` as a humantime string.
mod humantime_duration {
    use std::time::Duration;

    use serde::Deserialize;
    use serde::Deserializer;
    use serde::Serializer;

    pub fn serialize<S: Serializer>(value: &Duration, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&humantime::format_duration(*value).to_string())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Duration, D::Error> {
        let raw = String::deserialize(deserializer)?;
        humantime::parse_duration(&raw).map_err(serde::de::Error::custom)
    }
}
