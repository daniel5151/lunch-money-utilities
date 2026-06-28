use serde::Deserialize;

/// The `[venmo]` section of the unified `lm_utils.toml`.
///
/// Holds only the two Plaid account names this tool reconciles. The Lunch Money
/// API key it previously carried in a private `[lunch_money]` table now lives in
/// the shared `[common].lm_api_key` ([`lm_common::config::CommonConfig`]).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub venmo_acct: String,
    pub bank_acct: String,
}
