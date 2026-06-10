use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::Serializer;
use std::fmt;

/// A case-insensitive wrapper around a currency code (e.g. USD, EUR, GBP)
/// that always normalizes to uppercase for internal comparisons and hashing,
/// but serializes to lowercase for compatibility with the Lunch Money API.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct Currency(String);

impl Currency {
    /// Creates a new `Currency` instance, converting the input to uppercase.
    pub fn new(code: impl AsRef<str>) -> Self {
        Self(code.as_ref().to_ascii_uppercase())
    }

    /// Returns the uppercase string representation of the currency.
    pub fn to_uppercase(&self) -> String {
        self.0.clone()
    }

    /// Returns the lowercase string representation of the currency.
    pub fn to_lowercase(&self) -> String {
        self.0.to_ascii_lowercase()
    }

    /// Returns a reference to the underlying normalized uppercase string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Currency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl<'de> Deserialize<'de> for Currency {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(Self::new(s))
    }
}

impl Serialize for Currency {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // Serialize to lowercase for Lunch Money compatibility
        serializer.serialize_str(&self.to_lowercase())
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_currency_normalization_and_equality() {
        let c1 = Currency::new("usd");
        let c2 = Currency::new("USD");
        let c3 = Currency::new("EUR");

        assert_eq!(c1, c2);
        assert_ne!(c1, c3);
        assert_eq!(c1.to_uppercase(), "USD");
        assert_eq!(c1.to_lowercase(), "usd");
        assert_eq!(c1.as_str(), "USD");
    }

    #[test]
    fn test_currency_display() {
        let c = Currency::new("eur");
        assert_eq!(c.to_string(), "EUR");
    }

    #[test]
    fn test_currency_serde() {
        let c = Currency::new("gbp");
        let serialized = serde_json::to_string(&c).unwrap();
        assert_eq!(serialized, "\"gbp\"");

        let deserialized: Currency = serde_json::from_str("\"GbP\"").unwrap();
        assert_eq!(deserialized, Currency::new("GBP"));
    }

    #[test]
    fn test_external_id_serde() {
        let ext_sw = ExternalId::Splitwise(12345);
        let serialized_sw = serde_json::to_string(&ext_sw).unwrap();
        assert_eq!(serialized_sw, "\"splitwise_12345\"");

        let deserialized_sw: ExternalId = serde_json::from_str("\"splitwise_12345\"").unwrap();
        assert_eq!(deserialized_sw, ExternalId::Splitwise(12345));

        let ext_delta = ExternalId::SplitwiseDelta(67890, 0);
        let serialized_delta = serde_json::to_string(&ext_delta).unwrap();
        assert_eq!(serialized_delta, "\"splitwise_67890_delta_0\"");

        let deserialized_delta: ExternalId =
            serde_json::from_str("\"splitwise_67890_delta_0\"").unwrap();
        assert_eq!(deserialized_delta, ExternalId::SplitwiseDelta(67890, 0));

        let ext_delta_1 = ExternalId::SplitwiseDelta(67890, 1);
        let serialized_delta_1 = serde_json::to_string(&ext_delta_1).unwrap();
        assert_eq!(serialized_delta_1, "\"splitwise_67890_delta_1\"");

        let deserialized_delta_1: ExternalId =
            serde_json::from_str("\"splitwise_67890_delta_1\"").unwrap();
        assert_eq!(deserialized_delta_1, ExternalId::SplitwiseDelta(67890, 1));

        let ext_other = ExternalId::Other("my_custom_id".to_string());
        let serialized_other = serde_json::to_string(&ext_other).unwrap();
        assert_eq!(serialized_other, "\"my_custom_id\"");

        let deserialized_other: ExternalId = serde_json::from_str("\"my_custom_id\"").unwrap();
        assert_eq!(
            deserialized_other,
            ExternalId::Other("my_custom_id".to_string())
        );
    }
}
