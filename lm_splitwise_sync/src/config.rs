use serde::Deserialize;

/// The `[splitwise]` section of the unified `lm_utils.toml`.
///
/// The Splitwise-identity fields (`api_key`, `user_id`, `ignored_groups`) are
/// flattened to the section root via [`SplitwiseConfig`], so existing
/// `config.splitwise.*` access paths keep working, while `custom_accounts`,
/// `sync`, and `categories` live as `[splitwise.*]` subtables. The shared Lunch
/// Money API key is no longer here — it moved to `[common].lm_api_key`
/// ([`lm_common::config::CommonConfig`]).
#[derive(Deserialize, Clone)]
pub struct Config {
    #[serde(flatten)]
    pub splitwise: SplitwiseConfig,
    /// Optional currency → manual-account overrides (was `[lunch_money.custom_accounts]`).
    #[serde(default)]
    pub custom_accounts:
        std::collections::HashMap<crate::api::Currency, lunch_money::core::ManualAccountId>,
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
