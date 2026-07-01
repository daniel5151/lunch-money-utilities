//! A minimal HTTP client for raw (untyped) Lunch Money API access.
//!
//! Returns every response as a [`serde_json::Value`] so no fields are lost
//! to schema gaps in the typed library.

use std::time::Duration;

use anstream::eprintln;
use anyhow::Context;

/// A raw HTTP client that preserves every field the API returns.
pub(crate) struct RawClient {
    http: reqwest::Client,
    api_key: String,
    base_url: String,
    max_retries: u32,
    initial_delay: Duration,
}

impl RawClient {
    pub fn new(
        http: reqwest::Client,
        api_key: String,
        base_url: String,
        max_retries: u32,
        initial_delay: Duration,
    ) -> Self {
        Self {
            http,
            api_key,
            base_url,
            max_retries,
            initial_delay,
        }
    }

    /// GET an endpoint, returning the full JSON response as a `Value`.
    pub async fn get(
        &self,
        endpoint: &str,
        query: &[(&str, &str)],
    ) -> anyhow::Result<serde_json::Value> {
        let url = format!("{}/{}", self.base_url.trim_end_matches('/'), endpoint);
        let mut attempts = 0u32;
        loop {
            let builder = self
                .http
                .get(&url)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .query(query);

            let res = builder.send().await.context("HTTP request failed")?;

            if res.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
                if attempts < self.max_retries {
                    attempts += 1;
                    let delay = if let Some(retry_after) =
                        res.headers().get(reqwest::header::RETRY_AFTER)
                    {
                        retry_after
                            .to_str()
                            .ok()
                            .and_then(|s| s.parse::<u64>().ok())
                            .map(Duration::from_secs)
                            .unwrap_or(self.initial_delay * 2_u32.pow(attempts - 1))
                    } else {
                        self.initial_delay * 2_u32.pow(attempts - 1)
                    };
                    use lm_common::style::*;
                    eprintln! {
                        "  {STYLE_WARNING}⏳ Rate-limited on {endpoint} — waiting {:.0}s before retry ({attempts}/{max})...{STYLE_WARNING:#}",
                        delay.as_secs_f64(),
                        max = self.max_retries,
                    };
                    tokio::time::sleep(delay).await;
                    continue;
                }
            }

            if !res.status().is_success() {
                let status = res.status();
                let body = res.text().await.unwrap_or_default();
                anyhow::bail!(
                    "Lunch Money API error on GET {} ({}): {}",
                    endpoint,
                    status,
                    body.trim()
                );
            }

            return res
                .json()
                .await
                .with_context(|| format!("Failed parsing JSON from GET {}", endpoint));
        }
    }

    /// Download bytes from an arbitrary URL (e.g. a signed attachment URL).
    pub async fn download_bytes(&self, url: &str) -> anyhow::Result<Vec<u8>> {
        let res = self
            .http
            .get(url)
            .send()
            .await
            .context("attachment download failed")?;

        if !res.status().is_success() {
            let status = res.status();
            let body = res.text().await.unwrap_or_default();
            anyhow::bail!("Download failed ({}): {}", status, body.trim());
        }

        Ok(res.bytes().await?.to_vec())
    }
}
