use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub lunch_money: LunchMoneyConfig,
    pub accounts: AccountsConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LunchMoneyConfig {
    pub api_key: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AccountsConfig {
    pub venmo_acct: String,
    pub bank_acct: String,
}

impl Config {
    /// Parse and validate a TOML config.
    pub fn from_toml_str(s: &str) -> anyhow::Result<Self> {
        toml::from_str::<Config>(s)
            .map_err(|e| anyhow::anyhow!("Malformed configuration file: {e}"))
    }
}
