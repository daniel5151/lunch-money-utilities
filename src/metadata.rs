use serde::Deserialize;
use serde::Serialize;

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MetadataKind {
    Import,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct LunchMoneyTxMetadata {
    pub kind: MetadataKind,
    pub original: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MaybeLunchMoneyTxMetadata {
    Expected(LunchMoneyTxMetadata),
    Unexpected(serde_json::Value),
}

impl<'de> Deserialize<'de> for MaybeLunchMoneyTxMetadata {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let val = serde_json::Value::deserialize(deserializer)?;
        if let Ok(expected) = serde_json::from_value::<LunchMoneyTxMetadata>(val.clone()) {
            return Ok(Self::Expected(expected));
        }
        Ok(Self::Unexpected(val))
    }
}
