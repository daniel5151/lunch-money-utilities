use serde::Deserialize;

#[derive(Deserialize, Clone)]
pub struct Config {
    pub splitwise: SplitwiseConfig,
    pub lunch_money: LunchMoneyConfig,
    #[serde(default)]
    pub categories: std::collections::HashMap<String, CategoryValue>,
    #[serde(default)]
    pub sync: SyncConfig,
}

#[derive(Deserialize, Clone, Default)]
#[serde(deny_unknown_fields)]
pub struct SyncConfig {
    pub loan_tag: Option<String>,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(untagged)]
pub enum CategoryValue {
    Id(u64),
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
                    n.to_lowercase() == ignored_name.to_lowercase()
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
    pub custom_accounts: std::collections::HashMap<crate::api::Currency, u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_parsing_ignored_groups() {
        let toml_content = r#"
            [splitwise]
            api_key = "test_key"
            user_id = 12345
            ignored_groups = [123, "Roommates", "456"]

            [lunch_money]
            api_key = "lm_key"
        "#;

        let config: Config = toml::from_str(toml_content).unwrap();
        assert_eq!(config.splitwise.ignored_groups.len(), 3);

        // Test matching logic
        // 1. Matches ID 123 (which was a number in TOML)
        assert!(config.splitwise.is_group_ignored(123, Some("Any Name")));
        // 2. Matches Name "Roommates" (which was a string in TOML) - exact
        assert!(config.splitwise.is_group_ignored(999, Some("Roommates")));
        // 3. Matches Name "roommates" (which was a string in TOML) - case-insensitive
        assert!(config.splitwise.is_group_ignored(999, Some("roommates")));
        // 4. Matches ID 456 (which was a string `"456"` in TOML but parses as ID)
        assert!(config.splitwise.is_group_ignored(456, Some("Any Name")));

        // 5. Does not match some other group
        assert!(!config.splitwise.is_group_ignored(789, Some("Family")));
    }
}
