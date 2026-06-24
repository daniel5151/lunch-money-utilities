//! Microsoft Corporation earnings-statement backend.
//!
//! Implemented in a later commit. Microsoft statements use a columnar
//! CURRENT vs YEAR-TO-DATE layout that plain-text extraction scrambles, so this
//! backend reconstructs columns from positioned glyph coordinates.

use super::ParsedPage;
use anyhow::Result;

pub fn parse_pdf(_pdf_path: &std::path::Path) -> Result<Vec<ParsedPage>> {
    anyhow::bail!("Microsoft payslip backend is not implemented yet")
}
