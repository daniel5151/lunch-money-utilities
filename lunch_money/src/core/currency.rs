use std::fmt;

use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::Serializer;

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
