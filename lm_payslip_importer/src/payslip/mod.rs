//! Payslip parsing, abstracted over the payroll provider that produced the PDF.
//!
//! Every backend reduces a payslip PDF to the same provider-agnostic
//! [`ParsedPage`] model (rows keyed by `"Amount"`), which the importer consumes
//! without caring which payroll system generated it. Backend-specific text
//! extraction and table reconstruction live in the per-provider submodules
//! ([`workday`], [`microsoft`]); everything shared — the data model, money/date
//! parsing, and the [`PayslipKind`] dispatcher — lives here.

pub mod microsoft;
pub mod workday;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use jiff::civil::Date;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::str::FromStr;

/// A single line item from a payslip table (one earning, tax, or deduction).
///
/// `values` is keyed by column name; every backend populates at least
/// `"Amount"` (the current-period amount), which is the only column the
/// importer consumes.
#[derive(Debug, Clone, Default)]
pub struct RowData {
    pub description: String,
    #[expect(dead_code)]
    pub dates: String,
    pub values: HashMap<String, Decimal>,
}

impl RowData {
    /// Current-period amount for this row, or `0.00` if absent.
    pub fn amount(&self) -> Decimal {
        self.values.get("Amount").copied().unwrap_or(Decimal::ZERO)
    }
}

/// One payslip (one page of a PDF), reduced to the four line-item sections plus
/// header metadata. Produced by every backend; consumed by the importer.
#[derive(Debug, Clone)]
#[expect(dead_code)]
pub struct ParsedPage {
    pub page_num: usize,
    pub check_date: Date,
    pub period_begin: Date,
    pub period_end: Date,
    pub net_pay: Decimal,
    pub earnings: Vec<RowData>,
    pub employee_taxes: Vec<RowData>,
    pub pre_tax_deductions: Vec<RowData>,
    pub post_tax_deductions: Vec<RowData>,
}

/// Which payroll provider produced a payslip PDF. Selects the parsing backend
/// and, in config, which `[backends.<kind>]` section applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PayslipKind {
    /// Workday-generated payslips (e.g. Meta). Plain-text extraction.
    Workday,
    /// Microsoft Corporation earnings statements. Positioned-glyph extraction
    /// to reconstruct the CURRENT vs YEAR-TO-DATE columnar layout.
    Microsoft,
}

impl PayslipKind {
    /// The config key / CLI spelling for this kind (`"workday"`, `"microsoft"`).
    pub fn as_str(self) -> &'static str {
        match self {
            PayslipKind::Workday => "workday",
            PayslipKind::Microsoft => "microsoft",
        }
    }

    /// Whether this provider represents RSU vests as separate $0 paychecks that
    /// must be reconstructed as gross-comp-minus-taxes (Workday), versus folding
    /// stock comp inline as explicit offsetting line items that already
    /// reconcile to net pay (Microsoft). Only Workday needs the RSU path.
    pub fn uses_rsu_reconstruction(self) -> bool {
        matches!(self, PayslipKind::Workday)
    }
}

impl std::fmt::Display for PayslipKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for PayslipKind {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        match s.trim().to_lowercase().as_str() {
            "workday" => Ok(PayslipKind::Workday),
            "microsoft" | "msft" => Ok(PayslipKind::Microsoft),
            other => Err(anyhow!(
                "unknown payslip kind {other:?} (expected 'workday' or 'microsoft')"
            )),
        }
    }
}

/// Parse every page of a payslip PDF into [`ParsedPage`]s, dispatching to the
/// backend for `kind`. Backends own their own extraction strategy (plain text
/// for Workday, positioned glyphs for Microsoft), so this is the single entry
/// point the rest of the program uses.
pub fn parse_pdf(pdf_path: &std::path::Path, kind: PayslipKind) -> Result<Vec<ParsedPage>> {
    match kind {
        PayslipKind::Workday => workday::parse_pdf(pdf_path),
        PayslipKind::Microsoft => microsoft::parse_pdf(pdf_path),
    }
}

/// Best-effort detection of which provider produced a PDF, by sniffing the
/// first page's text for a provider fingerprint. Lets the importer accept a PDF
/// without the user having to restate the kind on every run; the result should
/// still be cross-checked against the configured kind.
pub fn detect_kind(pdf_path: &std::path::Path) -> Result<Option<PayslipKind>> {
    let pages = pdf_extract::extract_text_by_pages(pdf_path)
        .context("Failed to extract text from PDF for provider detection")?;
    let head = pages.first().map(|s| s.as_str()).unwrap_or("");
    let upper = head.to_uppercase();
    if upper.contains("MICROSOFT CORPORATION") {
        return Ok(Some(PayslipKind::Microsoft));
    }
    // Workday payslips carry the "Pay Period Begin ... Check Date" header band.
    if head.contains("Pay Period Begin") || head.contains("Pay Period End") {
        return Ok(Some(PayslipKind::Workday));
    }
    Ok(None)
}

/// Parse a payslip money token (e.g. `40,224.36`, `1,234.50-`) into a
/// [`Decimal`]. Workday renders credits with a *trailing* minus sign, which
/// [`Decimal::from_str`] does not accept, so it is normalised to a leading sign
/// first. A token that still fails to parse is surfaced as an error rather than
/// silently collapsing to `0.00`, which would let a botched extraction sail
/// through reconciliation (see audit finding #7).
pub fn clean_decimal(val: &str) -> Result<Decimal> {
    let mut clean = val.replace(',', "").trim().to_string();
    if clean.ends_with('-') {
        clean = format!("-{}", &clean[..clean.len() - 1]);
    }
    Decimal::from_str(&clean).with_context(|| format!("malformed decimal amount: {val:?}"))
}

pub fn parse_date_str(s: &str) -> Result<Date> {
    let parts: Vec<&str> = s.split('/').collect();
    if parts.len() != 3 {
        return Err(anyhow!("Invalid date format: {}", s));
    }
    let month = parts[0].parse::<i8>()?;
    let day = parts[1].parse::<i8>()?;
    let year = parts[2].parse::<i16>()?;
    Date::new(year, month, day).context("Failed to build Jiff Date")
}
