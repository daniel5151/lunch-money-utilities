use serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize, Clone)]
pub struct Config {
    pub lunch_money: LunchMoneyConfig,
    pub workday: WorkdayConfig,
    pub mapping: HashMap<String, String>,
    #[serde(default)]
    pub imputed_income: ImputedIncomeConfig,
}

#[derive(Deserialize, Default, Clone)]
#[serde(deny_unknown_fields)]
pub struct ImputedIncomeConfig {
    #[serde(default)]
    pub exceptions: Vec<String>,
}

#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct LunchMoneyConfig {
    pub api_key: Option<String>,
    pub net_zero_account: String,
    pub rsu_account: String,
}

#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct WorkdayConfig {
    pub payee_match: String,
    pub direct_deposit_payee: String,
    pub rsu_vest_payee: String,
}
