use serde::Deserialize;

#[derive(Deserialize, Clone)]
pub struct Config {
    pub splitwise: SplitwiseConfig,
    pub lunch_money: LunchMoneyConfig,
    #[serde(default)]
    pub categories: std::collections::HashMap<String, CategoryValue>,
    pub sync: SyncConfig,
}

#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct SyncConfig {
    pub loan_tag: Option<String>,
    pub backdated_tag: String,
    pub updated_tag: String,
    pub orphaned_tag: String,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(untagged)]
pub enum CategoryValue {
    Id(lunch_money::core::CategoryId),
    Name(String),
}

#[derive(Deserialize, Clone, Debug)]
#[serde(untagged)]
pub enum IgnoredGroup {
    Id(u64),
    Name(String),
}

impl IgnoredGroup {
    pub fn matches(&self, id: u64, name: Option<&str>) -> bool {
        match self {
            IgnoredGroup::Id(ignored_id) => *ignored_id == id,
            IgnoredGroup::Name(ignored_name) => {
                if let Ok(parsed_id) = ignored_name.parse::<u64>() {
                    if parsed_id == id {
                        return true;
                    }
                }
                if let Some(n) = name {
                    n == ignored_name
                } else {
                    false
                }
            }
        }
    }
}

#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct SplitwiseConfig {
    pub api_key: String,
    pub user_id: u64,
    #[serde(default)]
    pub ignored_groups: Vec<IgnoredGroup>,
}

impl SplitwiseConfig {
    pub fn is_group_ignored(&self, id: u64, name: Option<&str>) -> bool {
        self.ignored_groups.iter().any(|ig| ig.matches(id, name))
    }
}

#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct LunchMoneyConfig {
    pub api_key: String,
    #[serde(default)]
    pub custom_accounts:
        std::collections::HashMap<crate::api::Currency, lunch_money::core::ManualAccountId>,
}
