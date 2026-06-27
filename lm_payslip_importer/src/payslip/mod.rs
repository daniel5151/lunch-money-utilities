//! Payslip parsing, abstracted over the payroll provider that produced the PDF.
//!
//! Every backend reduces a payslip PDF to the same provider-agnostic
//! [`ParsedPage`] model (rows keyed by `"Amount"`), which the importer consumes
//! without caring which payroll system generated it. Backend-specific text
//! extraction and table reconstruction live in the per-provider submodules
//! ([`workday`], [`microsoft`]); everything shared — the data model, money/date
//! parsing, and the [`PayslipKind`] dispatcher — lives here.

pub mod adp_microsoft;
pub mod microsoft;
pub mod workday;

use std::collections::HashMap;
use std::str::FromStr;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use jiff::civil::Date;
use rust_decimal::Decimal;

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PayslipKind {
    /// Workday-generated payslips (e.g. Meta). Plain-text extraction.
    Workday,
    /// Microsoft Corporation earnings statements ("Official Copy"). Inline
    /// CURRENT vs YEAR-TO-DATE layout, bucketed by printed sign.
    Microsoft,
    /// ADP-generated Microsoft Corporation earnings statements. Two-physical-
    /// column layout that `pdf-extract` interleaves; deinterleaved by indentation.
    AdpMicrosoft,
}

impl PayslipKind {
    /// The config key / CLI spelling for this kind (`"workday"`, `"microsoft"`).
    pub fn as_str(self) -> &'static str {
        match self {
            PayslipKind::Workday => "workday",
            PayslipKind::Microsoft => "microsoft",
            PayslipKind::AdpMicrosoft => "adp_microsoft",
        }
    }

    /// Whether this provider represents RSU vests as separate $0 paychecks that
    /// must be reconstructed as gross-comp-minus-taxes (Workday), versus folding
    /// stock comp inline as explicit offsetting line items that already
    /// reconcile to net pay (Microsoft). Only Workday needs the RSU path.
    pub fn uses_rsu_reconstruction(self) -> bool {
        matches!(self, PayslipKind::Workday)
    }

    /// Whether this provider lists some taxable items as *one-sided* earnings
    /// add-backs (imputed income: group-term life, relocation gross-ups) that
    /// inflate gross pay without being paid in cash. Such a paycheck only
    /// reconciles to net pay once the importer injects an equal, opposite offset
    /// for each imputed line. Only Workday does this: Microsoft and ADP-Microsoft
    /// already print both halves of every non-cash item inline (e.g. `STOCK AWARD
    /// INCOME +/-`), so they reconcile on their own and an injected offset would
    /// double-count.
    pub fn injects_imputed_offsets(self) -> bool {
        matches!(self, PayslipKind::Workday)
    }

    /// Whether `description` is an imputed-income line for this provider, given
    /// the backend's configured extra descriptions (lines that are imputed but
    /// carry no machine-readable marker). Backends that do not inject offsets
    /// always return `false`; the per-provider marker convention lives in the
    /// backend module ([`workday::is_imputed_income`]).
    pub fn is_imputed_income(self, description: &str, extra: &[String]) -> bool {
        match self {
            PayslipKind::Workday => workday::is_imputed_income(description, extra),
            PayslipKind::Microsoft | PayslipKind::AdpMicrosoft => false,
        }
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
            "adp_microsoft" | "adp-microsoft" | "adp_msft" | "adp" => Ok(PayslipKind::AdpMicrosoft),
            other => Err(anyhow!(
                "unknown payslip kind {other:?} (expected 'workday', 'microsoft', or 'adp_microsoft')"
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
        PayslipKind::AdpMicrosoft => adp_microsoft::parse_pdf(pdf_path),
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
    let head = head.replace('\u{0}', "");
    let head = head.as_str();
    let upper = head.to_uppercase();
    if upper.contains("MICROSOFT CORPORATION") {
        // Two Microsoft formats share this string. The ADP-generated one carries
        // the ADP footer / its distinctive "Period Beg/End:" header band; the
        // in-house "Official Copy" does not.
        if upper.contains("AUTOMATICDATA PROCESSING")
            || upper.contains("AUTOMATIC DATA PROCESSING")
            || head.contains("Period Beg/End")
        {
            return Ok(Some(PayslipKind::AdpMicrosoft));
        }
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

#[cfg(test)]
mod recon_tests {
    //! Reconciliation tests over a real corpus. The corpus is private payroll
    //! data that does not live in the repo, so each test reads its path from an
    //! environment variable and is a no-op when that variable is unset:
    //!
    //! * `LM_MSFT_PDF`     — the multi-page "Official Copy" Microsoft PDF.
    //! * `LM_ADP_MSFT_DIR` — a directory of ADP-Microsoft `*.pdf` statements.
    //! * `LM_WORKDAY_DIR`  — a directory of Workday `*.pdf` payslips.
    //!
    //! Every parsed page must satisfy `earnings − taxes − pre_tax − post_tax ==
    //! net_pay` to within a cent, which is the same invariant the importer
    //! enforces before writing transactions. Workday is the exception: it lists
    //! imputed income (group-term life, relocation gross-ups) as one-sided
    //! earnings add-backs, so its pages only reconcile once those imputed
    //! earnings are removed — exactly the offset the importer injects.

    use rust_decimal::Decimal;

    use super::*;

    fn sum(rows: &[RowData]) -> Decimal {
        rows.iter().map(|r| r.amount()).sum()
    }

    fn assert_page_reconciles(p: &ParsedPage, ctx: &str) {
        let recon = sum(&p.earnings)
            - sum(&p.employee_taxes)
            - sum(&p.pre_tax_deductions)
            - sum(&p.post_tax_deductions);
        let diff = (recon - p.net_pay).abs();
        assert!(
            diff <= Decimal::new(1, 2),
            "{ctx} page {}: reconstructed {recon} != net {} (diff {})",
            p.page_num,
            p.net_pay,
            recon - p.net_pay
        );
    }

    #[test]
    fn microsoft_official_copy_reconciles() {
        let Ok(path) = std::env::var("LM_MSFT_PDF") else {
            eprintln!("LM_MSFT_PDF unset; skipping");
            return;
        };
        let path = std::path::Path::new(&path);
        let kind = detect_kind(path).unwrap();
        assert_eq!(kind, Some(PayslipKind::Microsoft), "detect_kind mismatch");
        let pages = microsoft::parse_pdf(path).expect("microsoft parse");
        assert!(!pages.is_empty(), "no pages parsed");
        for p in &pages {
            assert_page_reconciles(p, "microsoft");
        }
        eprintln!("microsoft: {} pages reconciled", pages.len());
    }

    #[test]
    fn adp_microsoft_corpus_reconciles() {
        let Ok(dir) = std::env::var("LM_ADP_MSFT_DIR") else {
            eprintln!("LM_ADP_MSFT_DIR unset; skipping");
            return;
        };
        let mut files: Vec<_> = std::fs::read_dir(&dir)
            .expect("read corpus dir")
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("pdf"))
            .collect();
        files.sort();
        assert!(!files.is_empty(), "no PDFs in {dir}");

        let mut total_pages = 0;
        for f in &files {
            let kind = detect_kind(f).unwrap();
            assert_eq!(
                kind,
                Some(PayslipKind::AdpMicrosoft),
                "detect_kind mismatch for {}",
                f.display()
            );
            let pages = adp_microsoft::parse_pdf(f)
                .unwrap_or_else(|e| panic!("parse {}: {e:#}", f.display()));
            assert!(!pages.is_empty(), "no pages parsed from {}", f.display());
            for p in &pages {
                assert_page_reconciles(p, &f.display().to_string());
            }
            total_pages += pages.len();
        }
        eprintln!(
            "adp_microsoft: {} files, {} statement pages reconciled",
            files.len(),
            total_pages
        );
    }

    #[test]
    fn workday_corpus_reconciles() {
        let Ok(dir) = std::env::var("LM_WORKDAY_DIR") else {
            eprintln!("LM_WORKDAY_DIR unset; skipping");
            return;
        };
        let mut files: Vec<_> = std::fs::read_dir(&dir)
            .expect("read corpus dir")
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("pdf"))
            .collect();
        files.sort();
        assert!(!files.is_empty(), "no PDFs in {dir}");

        // The unmarked imputed companions present in the Meta corpus (the
        // relocation tax-benefit gross-ups Workday does not asterisk). Marked
        // imputed lines (`*Imp GTL`, `*Relo Qualified`) are detected by prefix.
        let extra = vec!["Relocation Tax Ben".to_string()];

        let mut total_pages = 0;
        for f in &files {
            let kind = detect_kind(f).unwrap();
            assert_eq!(
                kind,
                Some(PayslipKind::Workday),
                "detect_kind mismatch for {}",
                f.display()
            );
            let pages =
                workday::parse_pdf(f).unwrap_or_else(|e| panic!("parse {}: {e:#}", f.display()));
            assert!(!pages.is_empty(), "no pages parsed from {}", f.display());
            for p in &pages {
                // RSU vest pages are routed through the separate
                // gross-comp-minus-taxes reconstruction (matched against a
                // brokerage transaction), not 4-section reconciliation, so skip
                // them here — they legitimately do not satisfy this invariant.
                if is_rsu_vest_page(p) {
                    continue;
                }
                assert_workday_page_reconciles(p, &extra, &f.display().to_string());
            }
            total_pages += pages.len();
        }
        eprintln!(
            "workday: {} files, {} statement pages reconciled",
            files.len(),
            total_pages
        );
    }

    /// Mirrors the importer's RSU-vest detection ([`rsu_vest_earning`] in
    /// `commands::import`): a page is an RSU vest if it carries a "restricted
    /// stock" earning with a non-zero current amount.
    fn is_rsu_vest_page(p: &ParsedPage) -> bool {
        p.earnings.iter().any(|r| {
            r.description.to_lowercase().contains("restricted stock") && !r.amount().is_zero()
        })
    }

    /// Workday-specific reconciliation: imputed earnings are one-sided add-backs
    /// that inflate gross without being paid, so they must be excluded from the
    /// earnings side for `earnings − taxes − pre − post == net_pay` to hold —
    /// mirroring the offset the importer injects for each imputed line.
    fn assert_workday_page_reconciles(p: &ParsedPage, extra: &[String], ctx: &str) {
        let cash_earnings: Decimal = p
            .earnings
            .iter()
            .filter(|r| !workday::is_imputed_income(&r.description, extra))
            .map(|r| r.amount())
            .sum();
        let recon = cash_earnings
            - sum(&p.employee_taxes)
            - sum(&p.pre_tax_deductions)
            - sum(&p.post_tax_deductions);
        let diff = (recon - p.net_pay).abs();
        assert!(
            diff <= Decimal::new(1, 2),
            "{ctx} page {}: reconstructed {recon} != net {} (diff {})",
            p.page_num,
            p.net_pay,
            recon - p.net_pay
        );
    }
}
