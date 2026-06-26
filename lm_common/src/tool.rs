//! The [`Tool`] trait and the shared-services [`ToolContext`] that every tool
//! is dispatched through by the `lm_utils` busybox binary.
//!
//! Layering rule: this module knows nothing about any specific tool. A tool
//! crate depends on `lm_common` and implements [`Tool`]; the binary references
//! the tools only through this trait plus its own static dispatch enum.

use std::future::Future;

use lunch_money::client::Client as LunchMoneyClient;
use lunch_money::client::TooManyRequestsPolicy;

use crate::lm_client;

/// Shared services handed to every tool's [`Tool::run`].
///
/// This is the generalization of Splitwise's former `AppContext`, reduced to
/// only the genuinely cross-tool pieces: a shared [`reqwest::Client`] (so
/// connection pooling/config is shared across tools), the global `--dry-run`
/// flag, and a factory for the Lunch Money client.
///
/// Deliberately tool-agnostic: it holds **no** tool-specific client. A tool
/// that needs a bespoke client (e.g. Splitwise's `splitwise::Client`, or
/// Splitwise's curated Lunch Money wrapper) constructs it inside its own
/// `run()` from [`ToolContext::http`]. The Lunch Money client is exposed as a
/// *factory* ([`ToolContext::lunch_money`]) rather than a pre-built client
/// because not every invocation has a key available (the payslip importer runs
/// keyless under `--dry-run`) and some tools want their own wrapper around it.
pub struct ToolContext {
    /// Shared HTTP client, reused by every tool and every API client.
    pub http: reqwest::Client,
    /// Whether the run is a dry run (the hoisted, uniform `--dry-run` flag).
    pub dry_run: bool,
}

impl ToolContext {
    /// Build a context with a fresh shared HTTP client.
    pub fn new(dry_run: bool) -> Self {
        Self {
            http: reqwest::Client::new(),
            dry_run,
        }
    }

    /// Build a Lunch Money client from the shared HTTP client, the given API
    /// key, and a 429 retry policy (accepts a
    /// [`RetryConfig`](crate::lm_client::RetryConfig) or a
    /// [`TooManyRequestsPolicy`]).
    pub fn lunch_money(
        &self,
        api_key: String,
        retry: impl Into<TooManyRequestsPolicy>,
    ) -> LunchMoneyClient {
        lm_client::build(self.http.clone(), api_key, retry)
    }
}

/// A single Lunch Money utility tool, dispatched by the `lm_utils` multiplexer.
///
/// Each tool crate exposes a unit struct implementing this trait. The binary
/// resolves a tool from either `argv[0]` (busybox symlink) or an explicit
/// subcommand, builds a [`ToolContext`], and calls [`Tool::run`].
pub trait Tool {
    /// Stable invocation name (argv0 basename / subcommand), e.g.
    /// `"venmo-balfixer"`.
    const NAME: &'static str;

    /// This tool's clap argument group (its subcommand tree).
    type Cli: clap::Args;

    /// Run the tool against the shared services and its parsed subcommand.
    fn run(
        cx: &ToolContext,
        cli: Self::Cli,
    ) -> impl Future<Output = anyhow::Result<()>>;
}
