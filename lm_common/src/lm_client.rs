//! The single place that constructs a Lunch Money API [`Client`], plus the
//! config-facing retry-policy type.
//!
//! Every tool previously built its own [`reqwest::Client`] and called
//! `lunch_money::client::Client::new` with a copy-pasted
//! `TooManyRequestsPolicy::Retry { max_retries: 5, initial_delay: 2s }`. This
//! module centralizes that construction and turns the retry knobs into a
//! serde-deserializable [`RetryConfig`] so they can be driven from config.

use std::time::Duration;

use lunch_money::client::Client;
use lunch_money::client::TooManyRequestsPolicy;

/// Configurable 429 (Too Many Requests) retry policy.
///
/// Deserializes from the `[common]` config table; when omitted the [`Default`]
/// reproduces the behavior every tool previously hardcoded: up to 5 retries
/// with an initial backoff of 2 seconds.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct RetryConfig {
    /// Maximum number of retry attempts after an initial 429 response.
    pub max_attempts: u32,
    /// Initial backoff delay, parsed/emitted as a humantime string (e.g. `"2s"`).
    #[serde(with = "humantime_duration")]
    pub initial_delay: Duration,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 5,
            initial_delay: Duration::from_secs(2),
        }
    }
}

impl From<RetryConfig> for TooManyRequestsPolicy {
    fn from(cfg: RetryConfig) -> Self {
        TooManyRequestsPolicy::Retry {
            max_retries: cfg.max_attempts,
            initial_delay: cfg.initial_delay,
        }
    }
}

/// Builds a Lunch Money [`Client`] from a shared HTTP client, an API key, and a
/// 429 retry policy.
///
/// `retry` accepts anything convertible into a [`TooManyRequestsPolicy`] — pass
/// a [`RetryConfig`] for the configurable retry behavior, or
/// [`TooManyRequestsPolicy::Fail`] to fail fast.
pub fn build(
    http: reqwest::Client,
    api_key: String,
    retry: impl Into<TooManyRequestsPolicy>,
) -> Client {
    Client::new(http, api_key, retry.into())
}

/// serde adapter that (de)serializes a [`Duration`] as a humantime string.
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
