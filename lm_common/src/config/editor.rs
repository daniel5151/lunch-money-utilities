//! Comment-preserving, in-place edits to the unified `lm_utils.toml` document.
//!
//! `init` flows only upsert sections they owns, leaving every other section —
//! and all of the inline comment pointers the wizards author — intact. The
//! helpers here splice a freshly-rendered section into an existing
//! [`DocumentMut`] while keeping comments and producing stable, readable
//! output.
//!
//! Two `toml_edit` quirks are handled so spliced sections render correctly:
//!
//! 1. **Position interleaving.** An independently-parsed section carries its
//!    own source positions; inserting it into another document would let the
//!    host document's later tables sort *between* the new section's header and
//!    its subtables. After every splice we walk the document and reassign
//!    positions in declaration order ([`renumber_positions`]) so each section's
//!    subtables stay contiguous beneath their parent.
//! 2. **Trailing trivia.** Comment-only lines at the very end of a parsed
//!    section (e.g. the commented example rows under `[splitwise.categories]`)
//!    become *document* trailing trivia rather than table content, and a
//!    `Table` has no trailing-trivia setter. We capture that trivia and
//!    re-attach it as the decor suffix of the section's deepest last subtable
//!    ([`attach_trailing`]) so those pointer comments survive the splice.

use std::path::Path;

use anyhow::Context;
use toml_edit::DocumentMut;
use toml_edit::Item;
use toml_edit::Table;

/// Reads `path` into an editable document, or returns a fresh empty one.
///
/// The entry point for an `init` wizard: if the unified config already exists
/// it is parsed (so `upsert_section` edits it in place, preserving sibling
/// sections and comments); otherwise a new empty document is returned.
pub fn read_or_new(path: &Path) -> anyhow::Result<DocumentMut> {
    if !path.exists() {
        return Ok(DocumentMut::new());
    }
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    content
        .parse::<DocumentMut>()
        .with_context(|| format!("Malformed config file {}", path.display()))
}

/// Inserts or replaces the section named `name` in `doc` from a rendered TOML
/// fragment, preserving the fragment's inline comments.
///
/// `section_toml` is a standalone TOML string whose top-level table is the
/// section being authored (e.g. it begins `[venmo]` / `[splitwise]` and may
/// contain nested `[splitwise.sync]` subtables and trailing comment lines). Any
/// existing section with the same name is removed first, so this is an upsert:
/// the first `init` inserts the section, a later `init` replaces it in place
/// while leaving sibling sections untouched.
pub fn upsert_section(doc: &mut DocumentMut, name: &str, section_toml: &str) -> anyhow::Result<()> {
    let parsed: DocumentMut = section_toml
        .parse()
        .with_context(|| format!("internal error: rendered [{name}] section is not valid TOML"))?;

    let trailing = parsed.trailing().as_str().unwrap_or_default().to_string();

    // Replace any existing section of the same name (upsert semantics).
    doc.as_table_mut().remove(name);

    // Before appending, re-home the host document's own trailing comment-only
    // lines onto the section they currently follow. On re-parse, a prior
    // section's trailing pointer comments become *document* trivia; without
    // this they would float past the section we are about to append and end up
    // stranded at the bottom of the file.
    rehome_doc_trailing(doc);

    let item =
        parsed.as_table().get(name).cloned().ok_or_else(|| {
            anyhow::anyhow!("internal error: rendered section is missing [{name}]")
        })?;
    doc.as_table_mut().insert(name, item);

    // Re-home the section's trailing comment-only lines (lost as document
    // trivia above) onto its deepest last subtable so they render in place.
    if !trailing.trim().is_empty() {
        if let Some(Item::Table(table)) = doc.as_table_mut().get_mut(name) {
            attach_trailing(table, &trailing);
        }
    }

    renumber_positions(doc.as_table_mut(), &mut 0);
    Ok(())
}

/// Ensures a `[common]` table exists with the given Lunch Money API key.
///
/// If `[common]` is absent it is created (with a brief explanatory comment) at
/// the top of the document; if it already exists only its `lm_api_key` value is
/// updated, leaving any retry settings and comments in place.
pub fn ensure_common_section(doc: &mut DocumentMut, lm_api_key: &str) {
    if !doc.as_table().contains_key("common") {
        let mut table = Table::new();
        table
            .decor_mut()
            .set_prefix("# Shared settings for every Lunch Money utility tool.\n");
        doc.as_table_mut().insert("common", Item::Table(table));
    }

    if let Some(Item::Table(table)) = doc.as_table_mut().get_mut("common") {
        table["lm_api_key"] = toml_edit::value(lm_api_key);
        if let Some(mut key) = table.key_mut("lm_api_key") {
            key.leaf_decor_mut()
                .set_prefix("# The single shared Lunch Money developer API key.\n");
        }
    }

    renumber_positions(doc.as_table_mut(), &mut 0);
}

/// Writes `doc` to `path` with `0600` permissions.
///
/// The unified config holds both the Lunch Money and Splitwise API keys, so the
/// file is created (or re-secured) read/write for the owner only.
pub fn write_secure(path: &Path, doc: &DocumentMut) -> anyhow::Result<()> {
    std::fs::write(path, doc.to_string())
        .with_context(|| format!("Failed to write config to {}", path.display()))?;
    set_owner_only_permissions(path)
        .with_context(|| format!("Failed to secure permissions on {}", path.display()))?;
    Ok(())
}

#[cfg(unix)]
fn set_owner_only_permissions(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn set_owner_only_permissions(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

/// Re-homes the document's trailing comment-only lines onto its last section.
///
/// When `upsert_section` re-parses an existing file, comment lines that trailed
/// the final section (e.g. the commented example rows under
/// `[splitwise.categories]`) are exposed as *document* trailing trivia rather
/// than as content of that section. Appending a new section would then leave
/// those comments stranded at the very bottom of the file, detached from the
/// section they document. Before appending, move that trivia onto the current
/// last top-level table's deepest last subtable so it stays in place.
fn rehome_doc_trailing(doc: &mut DocumentMut) {
    let trailing = doc.trailing().as_str().unwrap_or_default().to_string();
    if trailing.trim().is_empty() {
        return;
    }

    let last_top = doc
        .as_table()
        .iter()
        .filter(|(_, item)| item.is_table())
        .map(|(key, _)| key.to_string())
        .last();

    if let Some(key) = last_top {
        if let Some(Item::Table(table)) = doc.as_table_mut().get_mut(&key) {
            attach_trailing(table, &trailing);
            doc.set_trailing("");
        }
    }
}

/// Reassigns table positions in depth-first declaration order.
///
/// Independently-parsed sections carry their own source positions; after
/// splicing one into a host document the raw positions can interleave (a host
/// section sorting between a spliced parent and its subtables). Renumbering in
/// iteration order — which is insertion/declaration order — restores contiguous
/// rendering with each subtable immediately under its parent.
fn renumber_positions(table: &mut Table, counter: &mut isize) {
    for (_key, item) in table.iter_mut() {
        if let Item::Table(child) = item {
            child.set_position(Some(*counter));
            *counter += 1;
            renumber_positions(child, counter);
        }
    }
}

/// Attaches captured trailing trivia to a table's deepest last subtable.
///
/// Comment-only lines at the end of a parsed section land as document trivia
/// rather than table content, and `Table` exposes no trailing-trivia setter, so
/// we append them to the suffix decor of the last (deepest) subtable — the
/// position they originally occupied in the rendered fragment.
fn attach_trailing(table: &mut Table, trailing: &str) {
    let last_subtable = table
        .iter()
        .filter(|(_, item)| item.is_table())
        .map(|(key, _)| key.to_string())
        .last();

    if let Some(key) = last_subtable {
        if let Some(Item::Table(child)) = table.get_mut(&key) {
            attach_trailing(child, trailing);
            return;
        }
    }

    let decor = table.decor_mut();
    let existing = decor.suffix().and_then(|s| s.as_str()).unwrap_or_default();
    let combined = format!("{existing}\n{}", trailing.trim_end());
    decor.set_suffix(combined);
}
