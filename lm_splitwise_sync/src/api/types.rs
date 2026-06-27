use std::fmt;

use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::Serializer;

/// A structured wrapper around transaction external IDs to distinguish
/// Splitwise transaction IDs from other generic external IDs.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ExternalId {
    /// A transaction synced from Splitwise with its corresponding numeric Splitwise expense ID.
    Splitwise(u64),
    /// A delta transaction synced from Splitwise with a required index.
    SplitwiseDelta(u64, usize),
    /// Any other custom or un-recognized external ID.
    Other(String),
}

impl fmt::Display for ExternalId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Splitwise(id) => write!(f, "splitwise_{}", id),
            Self::SplitwiseDelta(id, idx) => write!(f, "splitwise_{}_delta_{}", id, idx),
            Self::Other(s) => write!(f, "{}", s),
        }
    }
}

impl<'de> Deserialize<'de> for ExternalId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        if let Some(id_str) = s.strip_prefix("splitwise_") {
            if let Some(pos) = id_str.find("_delta_") {
                let num_str = &id_str[..pos];
                let index_str = &id_str[pos + 7..];
                if let (Ok(id), Ok(idx)) = (num_str.parse::<u64>(), index_str.parse::<usize>()) {
                    return Ok(Self::SplitwiseDelta(id, idx));
                }
            } else if let Ok(id) = id_str.parse::<u64>() {
                return Ok(Self::Splitwise(id));
            }
        }
        Ok(Self::Other(s))
    }
}

impl Serialize for ExternalId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}
