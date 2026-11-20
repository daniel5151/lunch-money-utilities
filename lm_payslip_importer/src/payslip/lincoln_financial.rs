//! Lincoln Financial Group (LFG) claim payment statement backend.
//!
//! Parses printable "Payment Details for Claim … Check …" PDFs issued by
//! Lincoln Financial Group (e.g. statutory disability / paid family leave
//! benefits). Each PDF page is one claim payment check.
//!
//! ## Extraction quirks
//!
//! `pdf-extract` preserves the statement's box-by-box reading order cleanly
//! (`Check Summary` → `Check Details` → `Wages` → `Taxes` → `Other Deductions`
//! → `Offsets`), without interleaving left and right columns line-by-line, but
//! introduces several artifacts this backend normalizes:
//!
//! - **Logo is an image:** the words "Lincoln Financial" do not appear in the
//!   extracted text stream, so provider detection keys on the claim banner
//!   (`Payment Details for Claim`) and payment date labels (`Begin Payment Date`
//!   / `End Payment Date`).
//! - **PDF ligatures:** `Gross Beneﬁt`, `Net Beneﬁts`, and `Oﬀsets` contain
//!   Unicode ligature codepoints (`\u{fb00}`–`\u{fb04}`) that are expanded to
//!   ASCII before matching.
//! - **Kerning-split headers:** bold headings are split by stray spaces
//!   (`C heck Summary`, `C heck D etails`, `Other D eductions`), so section
//!   headers and known field labels are matched whitespace-insensitively.
//! - **Glued tokens:** the right-aligned check date in the claim banner is
//!   glued directly onto `Payment Details for Claim`
//!   (`11/16/2026Payment Details for Claim ...`), and the bottom `Other
//!   Deductions` body can share a line with the `Offsets` heading
//!   (`N/A Offsets`).
//! - **Whole-dollar currency tokens:** amounts carry a leading `$` and omit
//!   `.00` for whole dollars (`$170`, `$0`, `$156.99`).
//!
//! ## Accounting model & reconciliation
//!
//! The `Check Details` summary encodes:
//!
//! ```text
//! Adjusted Gross = Gross Benefit - Offsets
//! Net Benefit    = Adjusted Gross + COLA - Taxes (Withholding) - Other Deductions
//! ```
//!
//! This maps directly onto [`ParsedPage`]'s four sections:
//! - `earnings`: `Gross Benefit`, plus `COLA` when present.
//! - `pre_tax_deductions`: itemized rows from `Offsets` (or a summary `Offsets`
//!   row if `Check Details` reports a non-zero offset with no itemized lines).
//! - `employee_taxes`: itemized rows from `Taxes` (`Federal`, `State`, `Local`,
//!   `OASDI`, `Medicare`, …).
//! - `post_tax_deductions`: itemized rows from `Other Deductions` (or a summary
//!   `Other Deductions` row if `Check Details` reports a non-zero deduction
//!   with no itemized lines).
//!
//! The `Wages` section lists informational taxable/non-taxable wage bases and
//! is skipped. Before returning a page, [`parse_page`] cross-checks every
//! available subtotal (`Taxes` `Total`, `Check Details` totals, `Check Summary`
//! gross/net, `Adjusted Gross`) and asserts that
//! `earnings − taxes − pre_tax − post_tax == net_pay` to within one cent.

use std::collections::HashMap;
use std::sync::LazyLock;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use jiff::civil::Date;
use regex::Regex;
use rust_decimal::Decimal;

use super::ParsedPage;
use super::RowData;
use super::clean_decimal;
use super::parse_date_str;

/// `MM/DD/YYYY` anywhere in a line. Intentionally omits `\b` because
/// `pdf-extract` glues the check date directly onto `Payment Details for Claim`
/// (`11/16/2026Payment Details ...`), where there is no word boundary between
/// the year digit and `P`.
static DATE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d{2}/\d{2}/\d{4}").unwrap());

/// Trailing dollar amount token at the end of a line, supporting optional
/// commas, optional cents (`$170` or `$156.99`), and leading/trailing/
/// parenthesized signs (`-$10`, `$-10`, `($10.50)`, `$10.50-`).
static TRAILING_DOLLAR_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(\(?-?\$-?[\d,]+(?:\.\d{1,2})?-?\)?)$").unwrap());

/// Matches a line in `Other Deductions` that has the right-column `Offsets`
/// section header glued onto the end (e.g. `N/A Offsets`), tolerating kerning
/// spaces inside `Offsets`.
static TRAILING_OFFSETS_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(.*?)\s+o\s*f\s*f\s*s\s*e\s*t\s*s$").unwrap());

const CENT_EPS: Decimal = Decimal::from_parts(1, 0, 0, false, 2); // 0.01

/// Expand standard Unicode ligatures (`ﬀ`, `ﬁ`, `ﬂ`, `ﬃ`, `ﬄ`) to ASCII and
/// strip any embedded NUL bytes emitted by `pdf-extract`.
fn normalize_ligatures(s: &str) -> String {
    s.replace('\u{0}', "")
        .replace('\u{fb00}', "ff")
        .replace('\u{fb01}', "fi")
        .replace('\u{fb02}', "fl")
        .replace('\u{fb03}', "ffi")
        .replace('\u{fb04}', "ffl")
}

/// Strip all whitespace and lowercase `s` so kerning-split headers like
/// `"C heck D etails"` match `"checkdetails"` reliably.
fn compact_lower(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// Collapse internal runs of whitespace to single spaces.
fn normalize_spaces(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Parse a Lincoln Financial dollar token (e.g. `$170`, `$156.99`, `$0`,
/// `-$10.50`, `($10.50)`) into a [`Decimal`].
fn parse_dollar_amount(tok: &str) -> Result<Decimal> {
    let mut s = tok.trim();
    let mut negate = false;

    if let Some(inner) = s.strip_prefix('(').and_then(|s| s.strip_suffix(')')) {
        negate = true;
        s = inner.trim();
    }
    if let Some(rest) = s.strip_prefix('-') {
        negate = !negate;
        s = rest.trim();
    }
    let Some(rest) = s.strip_prefix('$') else {
        return Err(anyhow!("expected dollar amount, got {tok:?}"));
    };
    s = rest.trim();
    if let Some(rest) = s.strip_prefix('-') {
        negate = !negate;
        s = rest.trim();
    }

    let mut val = clean_decimal(s).with_context(|| format!("dollar amount {tok:?}"))?;
    if negate {
        val = -val;
    }
    Ok(val)
}

/// Split a `Label $Amount` line into `(trimmed_label, amount)`. Returns `None`
/// if the line does not end with a dollar token (headers, `N/A`, blank-value
/// summary rows, non-currency metadata like `Check Number 120542674`).
fn split_label_and_dollar_amount(line: &str) -> Result<Option<(&str, Decimal)>> {
    let trimmed = line.trim();
    let Some(caps) = TRAILING_DOLLAR_RE.captures(trimmed) else {
        return Ok(None);
    };
    let m = caps.get(1).unwrap();
    let tok = m.as_str();
    // Reject unbalanced parentheses (e.g. a label ending in `(foo)` without a
    // matching paren around the dollar token).
    if tok.starts_with('(') != tok.ends_with(')') {
        return Ok(None);
    }
    let label = trimmed[..m.start()].trim();
    if label.is_empty() {
        return Ok(None);
    }
    let amount = parse_dollar_amount(tok).with_context(|| format!("line {trimmed:?}"))?;
    Ok(Some((label, amount)))
}

fn make_row(description: impl Into<String>, amount: Decimal) -> RowData {
    let mut values = HashMap::new();
    values.insert("Amount".to_string(), amount);
    RowData {
        description: description.into(),
        dates: String::new(),
        values,
    }
}

fn sum_rows(rows: &[RowData]) -> Decimal {
    rows.iter().map(|r| r.amount()).sum()
}

fn assert_close(actual: Decimal, expected: Decimal, page_num: usize, label: &str) -> Result<()> {
    let diff = (actual - expected).abs();
    if diff > CENT_EPS {
        anyhow::bail!(
            "Page {page_num}: Lincoln Financial {label} mismatch: got {actual}, expected {expected} (diff {})",
            actual - expected
        );
    }
    Ok(())
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum Section {
    Header,
    CheckSummary,
    CheckDetails,
    Wages,
    Taxes,
    OtherDeductions,
    Offsets,
}

#[derive(Default)]
struct CheckDetailsSummary {
    gross: Option<Decimal>,
    offsets: Option<Decimal>,
    adjusted_gross: Option<Decimal>,
    cola: Option<Decimal>,
    taxes: Option<Decimal>,
    other_deductions: Option<Decimal>,
    net: Option<Decimal>,
}

/// Parse one page of a Lincoln Financial Group claim payment PDF into a
/// [`ParsedPage`].
pub fn parse_page(page_text: &str, page_num: usize) -> Result<ParsedPage> {
    let normalized = normalize_ligatures(page_text);

    let mut check_date: Option<Date> = None;
    let mut period_begin: Option<Date> = None;
    let mut period_end: Option<Date> = None;

    let mut summary_gross: Option<Decimal> = None;
    let mut summary_net: Option<Decimal> = None;
    let mut details = CheckDetailsSummary::default();

    let mut employee_taxes: Vec<RowData> = Vec::new();
    let mut taxes_total: Option<Decimal> = None;

    let mut post_tax_deductions: Vec<RowData> = Vec::new();
    let mut other_deductions_total: Option<Decimal> = None;

    let mut pre_tax_deductions: Vec<RowData> = Vec::new();
    let mut offsets_total: Option<Decimal> = None;

    let mut section = Section::Header;

    let handle_other_deductions_line =
        |line: &str, rows: &mut Vec<RowData>, total: &mut Option<Decimal>| -> Result<()> {
            if let Some((label, amount)) = split_label_and_dollar_amount(line)? {
                if compact_lower(label) == "total" {
                    *total = Some(amount);
                } else {
                    rows.push(make_row(normalize_spaces(label), amount));
                }
            }
            Ok(())
        };

    for raw_line in normalized.lines() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let compact = compact_lower(trimmed);

        // Claim payment banner carries the check date (often glued directly to
        // the start of "Payment Details for Claim ...").
        if compact.contains("paymentdetailsforclaim") {
            if let Some(m) = DATE_RE.find(trimmed) {
                check_date = Some(
                    parse_date_str(m.as_str())
                        .with_context(|| format!("page {page_num}: check date in {trimmed:?}"))?,
                );
            }
            continue;
        }

        // Section transitions. Note that `Check Details` contains summary rows
        // literally titled "Offsets" and "Other Deductions" (which may be
        // blank), so those two names only transition to the bottom sections
        // once we have moved past `CheckDetails`.
        match compact.as_str() {
            "checksummary" => {
                section = Section::CheckSummary;
                continue;
            }
            "checkdetails" => {
                section = Section::CheckDetails;
                continue;
            }
            "wages" => {
                section = Section::Wages;
                continue;
            }
            "taxes" => {
                section = Section::Taxes;
                continue;
            }
            "otherdeductions" if section != Section::CheckDetails => {
                section = Section::OtherDeductions;
                continue;
            }
            "offsets" if section != Section::CheckDetails => {
                section = Section::Offsets;
                continue;
            }
            _ => {}
        }

        match section {
            Section::Header | Section::Wages => {}
            Section::CheckSummary => {
                if compact.starts_with("beginpaymentdate") {
                    if let Some(m) = DATE_RE.find(trimmed) {
                        period_begin = Some(parse_date_str(m.as_str()).with_context(|| {
                            format!("page {page_num}: begin payment date in {trimmed:?}")
                        })?);
                    }
                } else if compact.starts_with("endpaymentdate") {
                    if let Some(m) = DATE_RE.find(trimmed) {
                        period_end = Some(parse_date_str(m.as_str()).with_context(|| {
                            format!("page {page_num}: end payment date in {trimmed:?}")
                        })?);
                    }
                } else if let Some((label, amount)) = split_label_and_dollar_amount(trimmed)? {
                    match compact_lower(label).as_str() {
                        "grossbenefits" | "grossbenefit" => summary_gross = Some(amount),
                        "netbenefits" | "netbenefit" => summary_net = Some(amount),
                        _ => {}
                    }
                }
            }
            Section::CheckDetails => {
                if let Some((label, amount)) = split_label_and_dollar_amount(trimmed)? {
                    match compact_lower(label).as_str() {
                        "grossbenefit" | "grossbenefits" => details.gross = Some(amount),
                        "offsets" => details.offsets = Some(amount),
                        "adjustedgross" => details.adjusted_gross = Some(amount),
                        "cola" => details.cola = Some(amount),
                        "taxes(withholding)" | "taxeswithholding" => details.taxes = Some(amount),
                        "otherdeductions" => details.other_deductions = Some(amount),
                        "netbenefit" | "netbenefits" => details.net = Some(amount),
                        _ => {}
                    }
                }
            }
            Section::Taxes => {
                if let Some((label, amount)) = split_label_and_dollar_amount(trimmed)? {
                    if compact_lower(label) == "total" {
                        taxes_total = Some(amount);
                    } else {
                        employee_taxes.push(make_row(normalize_spaces(label), amount));
                    }
                }
            }
            Section::OtherDeductions => {
                // `pdf-extract` can glue the right-column `Offsets` heading onto
                // the same line as the last `Other Deductions` entry (e.g.
                // `"N/A Offsets"`). Split it so both sections parse cleanly.
                if let Some(caps) = TRAILING_OFFSETS_RE.captures(trimmed) {
                    let prefix = caps.get(1).unwrap().as_str().trim();
                    if !prefix.is_empty() {
                        handle_other_deductions_line(
                            prefix,
                            &mut post_tax_deductions,
                            &mut other_deductions_total,
                        )?;
                    }
                    section = Section::Offsets;
                } else {
                    handle_other_deductions_line(
                        trimmed,
                        &mut post_tax_deductions,
                        &mut other_deductions_total,
                    )?;
                }
            }
            Section::Offsets => {
                if let Some((label, amount)) = split_label_and_dollar_amount(trimmed)? {
                    if compact_lower(label) == "total" {
                        offsets_total = Some(amount);
                    } else {
                        pre_tax_deductions.push(make_row(normalize_spaces(label), amount));
                    }
                }
            }
        }
    }

    let check_date = check_date
        .ok_or_else(|| anyhow!("Page {page_num}: Lincoln Financial check date not found"))?;
    let period_begin = period_begin.unwrap_or(check_date);
    let period_end = period_end.unwrap_or(check_date);

    if let (Some(sg), Some(dg)) = (summary_gross, details.gross) {
        assert_close(dg, sg, page_num, "Gross Benefit vs Check Summary")?;
    }
    if let (Some(sn), Some(dn)) = (summary_net, details.net) {
        assert_close(dn, sn, page_num, "Net Benefit vs Check Summary")?;
    }

    let gross_benefit = details
        .gross
        .or(summary_gross)
        .ok_or_else(|| anyhow!("Page {page_num}: Lincoln Financial Gross Benefit not found"))?;
    let net_pay = details
        .net
        .or(summary_net)
        .ok_or_else(|| anyhow!("Page {page_num}: Lincoln Financial Net Benefit not found"))?;

    let mut earnings = vec![make_row("Gross Benefit", gross_benefit)];
    if let Some(cola) = details.cola {
        earnings.push(make_row("COLA", cola));
    }

    // Cross-check Taxes section against its own `Total` and `Check Details`.
    let taxes_sum = sum_rows(&employee_taxes);
    if let Some(expected) = taxes_total {
        assert_close(taxes_sum, expected, page_num, "Taxes total")?;
    }
    if let Some(expected) = details.taxes {
        assert_close(
            taxes_sum,
            expected,
            page_num,
            "Taxes vs Check Details Taxes (Withholding)",
        )?;
    }

    // Reconcile Offsets (pre-tax reductions from Gross Benefit -> Adjusted Gross).
    if !pre_tax_deductions.is_empty() {
        let offsets_sum = sum_rows(&pre_tax_deductions);
        if let Some(expected) = offsets_total {
            assert_close(offsets_sum, expected, page_num, "Offsets total")?;
        }
        if let Some(expected) = details.offsets {
            assert_close(
                offsets_sum,
                expected,
                page_num,
                "Offsets vs Check Details Offsets",
            )?;
        }
    } else if let Some(amt) = details.offsets {
        if !amt.is_zero() {
            pre_tax_deductions.push(make_row("Offsets", amt));
        }
    }

    if let Some(expected_adj) = details.adjusted_gross {
        let offsets_sum = sum_rows(&pre_tax_deductions);
        assert_close(
            gross_benefit - offsets_sum,
            expected_adj,
            page_num,
            "Adjusted Gross",
        )?;
    }

    // Reconcile Other Deductions (post-tax reductions).
    if !post_tax_deductions.is_empty() {
        let ded_sum = sum_rows(&post_tax_deductions);
        if let Some(expected) = other_deductions_total {
            assert_close(ded_sum, expected, page_num, "Other Deductions total")?;
        }
        if let Some(expected) = details.other_deductions {
            assert_close(
                ded_sum,
                expected,
                page_num,
                "Other Deductions vs Check Details",
            )?;
        }
    } else if let Some(amt) = details.other_deductions {
        if !amt.is_zero() {
            post_tax_deductions.push(make_row("Other Deductions", amt));
        }
    }

    // Final invariant: earnings - taxes - pre_tax - post_tax == net_pay.
    let recon = sum_rows(&earnings)
        - sum_rows(&employee_taxes)
        - sum_rows(&pre_tax_deductions)
        - sum_rows(&post_tax_deductions);
    assert_close(recon, net_pay, page_num, "net benefit reconciliation")?;

    Ok(ParsedPage {
        page_num,
        check_date,
        period_begin,
        period_end,
        net_pay,
        earnings,
        employee_taxes,
        pre_tax_deductions,
        post_tax_deductions,
    })
}

/// Parse every non-empty page of a Lincoln Financial Group claim payment PDF
/// into [`ParsedPage`]s. Dispatched from [`super::parse_pdf`].
pub fn parse_pdf(pdf_path: &std::path::Path) -> Result<Vec<ParsedPage>> {
    let pages =
        pdf_extract::extract_text_by_pages(pdf_path).context("Failed to extract text from PDF")?;

    let mut parsed = Vec::new();
    for (i, page_text) in pages.iter().enumerate() {
        let page_num = i + 1;
        if page_text.trim().is_empty() {
            continue;
        }
        parsed.push(parse_page(page_text, page_num)?);
    }
    Ok(parsed)
}
