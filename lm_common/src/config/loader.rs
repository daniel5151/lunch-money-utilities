//! Loading helpers for the unified `lm_utils.toml` document and its typed sections.
//!
//! Provides helpers to pull individual sections out of a parsed [`DocumentMut`]
//! as typed values. Section extraction is generic: it clones the named table
//! into a standalone document and runs `serde` over it, so a tool's `Config`
//! type can be any `DeserializeOwned` shape (including nested subtables and
//! `#[serde(flatten)]`).

use anyhow::Context;
use toml_edit::DocumentMut;

use super::CommonConfig;

/// The single unified config filename shared by every tool.
pub const DEFAULT_CONFIG_FILENAME: &str = "lm_utils.toml";

/// Deserializes the `[common]` table out of the document.
///
/// A missing `[common]` table yields [`CommonConfig::default`] (no key, default
/// retry) so tools that can run keyless under `--dry-run` still load.
pub fn common_section(doc: &DocumentMut) -> anyhow::Result<CommonConfig> {
    match optional_section::<CommonConfig>(doc, "common")? {
        Some(common) => Ok(common),
        None => Ok(CommonConfig::default()),
    }
}

/// Deserializes the named section as `T`, returning `None` if it is absent.
pub fn optional_section<T: serde::de::DeserializeOwned>(
    doc: &DocumentMut,
    name: &str,
) -> anyhow::Result<Option<T>> {
    let Some(item) = doc.get(name) else {
        return Ok(None);
    };
    let table = item
        .as_table()
        .ok_or_else(|| anyhow::anyhow!("[{name}] is not a table in {DEFAULT_CONFIG_FILENAME}"))?;

    let mut sub = DocumentMut::new();
    *sub.as_table_mut() = table.clone();
    let value = toml_edit::de::from_document(sub)
        .with_context(|| format!("Malformed [{name}] section in {DEFAULT_CONFIG_FILENAME}"))?;
    Ok(Some(value))
}
