use serde::Deserialize;

#[derive(Deserialize, Clone)]
pub struct Config {
    pub splitwise: SplitwiseConfig,
    pub lunch_money: LunchMoneyConfig,
}

#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct SplitwiseConfig {
    pub api_key: String,
    pub user_id: u64,
    #[serde(default)]
    pub ignored_groups: Vec<u64>,
}

#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct LunchMoneyConfig {
    pub api_key: String,
    pub target_accounts: std::collections::HashMap<String, u64>,
}
