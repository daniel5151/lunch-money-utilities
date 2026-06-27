//! Loading the unified `lm_utils.toml` document and extracting typed sections.
//!
//! The loader searches the current working directory and then the running
//! executable's directory (matching the per-tool loaders it replaces), parses
//! the file into a [`DocumentMut`] so comments/ordering survive a later
//! `init` rewrite, and exposes helpers to pull individual sections out as typed
//! values. Section extraction is generic: it clones the named table into a
//! standalone document and runs `serde` over it, so a tool's `Config` type can
//! be any `DeserializeOwned` shape (including nested subtables and
//! `#[serde(flatten)]`).

use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use toml_edit::DocumentMut;

use super::CommonConfig;

/// The single unified config filename shared by every tool.
pub const DEFAULT_CONFIG_FILENAME: &str = "lm_utils.toml";

/// Locates and parses `lm_utils.toml` into an editable document.
///
/// Searches the current working directory first, then the directory of the
/// running executable. Returns the parsed [`DocumentMut`] together with the
/// path it was read from (useful for writing the file back after an `init`
/// upsert). Fails with a `run init` hint when no config file is found.
pub fn load_document() -> anyhow::Result<(DocumentMut, PathBuf)> {
    let filename = Path::new(DEFAULT_CONFIG_FILENAME);

    // 1. Current working directory.
    if filename.exists() {
        return parse_at(filename);
    }

    // 2. Directory of the running executable.
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let candidate = exe_dir.join(filename);
            if candidate.exists() {
                return parse_at(&candidate);
            }
        }
    }

    anyhow::bail!(
        "Configuration file '{DEFAULT_CONFIG_FILENAME}' not found in current directory or \
         executable directory. Run the relevant tool's `init` subcommand to generate one \
         (e.g. `lm-utils venmo-balfixer init`)."
    )
}

pub fn resolve_config_path(user_path: Option<&Path>) -> PathBuf {
    if let Some(path) = user_path {
        return path.to_path_buf();
    }

    let filename = Path::new(DEFAULT_CONFIG_FILENAME);

    // 1. Current working directory.
    if filename.exists() {
        return filename.to_path_buf();
    }

    // 2. Directory of the running executable.
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let candidate = exe_dir.join(filename);
            if candidate.exists() {
                return candidate;
            }
        }
    }

    filename.to_path_buf()
}

pub fn parse_at(path: &Path) -> anyhow::Result<(DocumentMut, PathBuf)> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file from {}", path.display()))?;
    let doc: DocumentMut = content
        .parse()
        .with_context(|| format!("Malformed config file {}", path.display()))?;
    Ok((doc, path.to_path_buf()))
}

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

/// Deserializes the named section as `T`, erroring if it is absent.
///
/// The section's table is cloned into a standalone [`DocumentMut`] and run
/// through `serde`, so `T` may be any shape — nested subtables and
/// `#[serde(flatten)]` are both supported.
pub fn deserialize_section<T: serde::de::DeserializeOwned>(
    doc: &DocumentMut,
    name: &str,
) -> anyhow::Result<T> {
    optional_section(doc, name)?.ok_or_else(|| {
        anyhow::anyhow!(
            "Missing [{name}] section in {DEFAULT_CONFIG_FILENAME}. Run `lm-utils {name} init` \
             to configure it."
        )
    })
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
