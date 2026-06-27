//! "Official Copy of Microsoft Corporation Earnings Statement" backend.
//!
//! Microsoft's in-house earnings statement (as opposed to the ADP-generated one
//! handled by [`super::adp_microsoft`]). Each PDF page is one pay period.
//!
//! ## Layout
//!
//! The statement prints a `CURRENT` block followed by a `YEAR-TO-DATE` block.
//! `pdf-extract` keeps each line intact but emits the current-period amount
//! *inline* with its description (`FEDERAL INCOME TAX -2,104.85`), then dumps
//! the YTD figures afterwards as bare, description-less numbers. This backend
//! reads only the `CURRENT` block — from the `DESCRIPTION RATE HOURS AMOUNT
//! TOTAL` header down to the `NET PAY` line — and stops before YTD (whose lines
//! have no description and are ignored anyway).
//!
//! The block is delimited by running subtotals:
//!
//! ```text
//! <earnings…>            REGULAR HRS, PERKS + TAXABLE
//! GROSS PAY
//! <adjustments…>         401K (PRE-TAX), STOCK AWARD INCOME, DISABILITY INS, HSA
//! TOTAL ADJUSTMENT TO EARNINGS
//! TAXABLE EARNINGS
//! <taxes…>               FEDERAL INCOME TAX, SOCIAL SECURITY TAX, MEDICARE TAX
//! TAXES WITHHELD
//! <after-tax…>           ESPP, LEGAL PLAN, STOCK AWARD INCOME (offset), ROTH, …
//! TOTAL AFTER TAX DEDUCTIONS
//! NET PAY
//! ```
//!
//! ## Sign convention and reconciliation
//!
//! Every current-period line item carries its own sign as printed: earnings and
//! imputed-income add-backs are positive, taxes and deductions are negative, and
//! a handful of lines flip (a stock-award-tax sell-to-cover refund prints
//! positive; the matching stock-award-income offset prints negative). The whole
//! statement reconciles as the plain sum of every line item == `NET PAY`.
//!
//! The importer wants four sections in a *positive-magnitude* convention
//! (earnings positive — it negates them to a credit; taxes / pre-tax / post-tax
//! positive as debits). The transformation that preserves the reconciliation is
//! simply to **bucket by printed sign**: a positive line is income (stored
//! `+value` in `earnings`); a negative line is a deduction (stored `+|value|`).
//! The structural block a negative line sits in selects which deduction section
//! it lands in (adjustments → pre-tax, taxes → employee taxes, after-tax →
//! post-tax) for categorisation only — the importer treats all three identically
//! for arithmetic. This makes `earnings − deductions == net_pay` hold exactly,
//! which [`parse_page`] asserts before returning. Naturally offsetting pairs
//! (stock award income +/-, imputed disability +/-) fall out across the
//! earnings/deduction split on their own, with no `*`-prefix offset injection.

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

/// A signed money token with thousands separators and a 2+ digit fraction.
static MONEY_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^-?[\d,]+\.\d{2,}$").unwrap());

fn is_money(tok: &str) -> bool {
    MONEY_RE.is_match(tok)
}

/// Month abbreviations as printed in the statement header (`Mar 31, 2025`).
const MONTHS: [&str; 12] = [
    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
];

/// Parse a `Mon DD, YYYY` date (e.g. `Mar 31, 2025`) into a [`Date`].
fn parse_month_name_date(s: &str) -> Result<Date> {
    let cleaned = s.replace(',', "");
    let parts: Vec<&str> = cleaned.split_whitespace().collect();
    if parts.len() != 3 {
        return Err(anyhow!("unexpected month-name date {s:?}"));
    }
    let mon_lc = parts[0].to_lowercase();
    let month = MONTHS
        .iter()
        .position(|m| mon_lc.starts_with(m))
        .ok_or_else(|| anyhow!("unknown month in date {s:?}"))? as i8
        + 1;
    let day = parts[1]
        .parse::<i8>()
        .with_context(|| format!("day in date {s:?}"))?;
    let year = parts[2]
        .parse::<i16>()
        .with_context(|| format!("year in date {s:?}"))?;
    Date::new(year, month, day).context("Failed to build Jiff Date")
}

/// Structural block within the CURRENT section, used to route a *negative* line
/// to the correct deduction section. Positive lines always become earnings.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum Block {
    /// Before any subtotal, and the adjustments-to-earnings block. Negative
    /// lines here (401k pre-tax, HSA) are pre-tax deductions.
    GrossAndAdjust,
    /// Between TAXABLE EARNINGS and TAXES WITHHELD: statutory taxes.
    Taxes,
    /// Between TAXES WITHHELD and TOTAL AFTER TAX DEDUCTIONS.
    AfterTax,
}

/// Subtotal / control lines that delimit the blocks and must never be parsed as
/// data rows.
fn is_control_line(t: &str) -> bool {
    const CONTROL: [&str; 6] = [
        "GROSS PAY",
        "TOTAL ADJUSTMENT TO EARNINGS",
        "TAXABLE EARNINGS",
        "TAXES WITHHELD",
        "TOTAL AFTER TAX DEDUCTIONS",
        "NET PAY",
    ];
    CONTROL.iter().any(|c| t.starts_with(c))
}

/// Extract `(description, current_amount)` from a CURRENT-block data line. The
/// current-period amount is the line's final money token; the description is the
/// leading run of non-money tokens. Returns `None` for a line with no money
/// token (a blank-current row, e.g. `PERKS + TAXABLE`).
fn parse_line(line: &str) -> Result<Option<(String, Decimal)>> {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    let last_money_idx = tokens.iter().rposition(|t| is_money(t));
    let Some(idx) = last_money_idx else {
        return Ok(None);
    };
    // Description = tokens before the first money/numeric token.
    let first_num = tokens
        .iter()
        .position(|t| is_money(t) || t.parse::<f64>().is_ok())
        .unwrap_or(idx);
    let description = tokens[..first_num].join(" ").trim().to_string();
    if description.is_empty() {
        return Ok(None);
    }
    let amount = clean_decimal(tokens[idx]).with_context(|| format!("msft line {line:?}"))?;
    Ok(Some((description, amount)))
}

fn row(description: String, amount: Decimal) -> RowData {
    let mut values = HashMap::new();
    values.insert("Amount".to_string(), amount);
    RowData {
        description,
        dates: String::new(),
        values,
    }
}

fn sum_amounts(rows: &[RowData]) -> Decimal {
    rows.iter().map(|r| r.amount()).sum()
}

/// Parse one statement page into a [`ParsedPage`], or `None` if the page has no
/// CURRENT/NET PAY block (a blank or non-statement page).
fn parse_page(page: &str, page_num: usize) -> Result<Option<ParsedPage>> {
    let page = page.replace('\u{0}', "");
    let lines: Vec<&str> = page.lines().map(|l| l.trim()).collect();

    // Header dates: each label is on its own line, value on the next non-empty.
    let next_value = |start: usize| -> Option<&str> {
        lines
            .iter()
            .skip(start + 1)
            .find(|l| !l.is_empty())
            .copied()
    };
    let mut check_date: Option<Date> = None;
    let mut period_begin: Option<Date> = None;
    let mut period_end: Option<Date> = None;
    for (i, l) in lines.iter().enumerate() {
        match *l {
            "Check Date" if check_date.is_none() => {
                if let Some(v) = next_value(i) {
                    check_date = Some(parse_month_name_date(v)?);
                }
            }
            "Period Begin Date" if period_begin.is_none() => {
                if let Some(v) = next_value(i) {
                    period_begin = Some(parse_month_name_date(v)?);
                }
            }
            "Period End Date" if period_end.is_none() => {
                if let Some(v) = next_value(i) {
                    period_end = Some(parse_month_name_date(v)?);
                }
            }
            _ => {}
        }
    }

    // Find the CURRENT block: from the column header to NET PAY.
    let header_idx = lines
        .iter()
        .position(|l| l.contains("DESCRIPTION") && l.contains("AMOUNT") && l.contains("TOTAL"));
    let Some(header_idx) = header_idx else {
        return Ok(None);
    };

    let mut earnings: Vec<RowData> = Vec::new();
    let mut employee_taxes: Vec<RowData> = Vec::new();
    let mut pre_tax_deductions: Vec<RowData> = Vec::new();
    let mut post_tax_deductions: Vec<RowData> = Vec::new();
    let mut net_pay: Option<Decimal> = None;
    // The CURRENT block of a complete statement always prints a `GROSS PAY`
    // subtotal between earnings and adjustments. Microsoft reprints the same
    // pay period a second time as a duplicate page, and on those reprints
    // `pdf-extract` intermittently drops the entire earnings section from the
    // text layer — the CURRENT block then jumps straight from the column header
    // to `FEDERAL INCOME TAX` with no earnings, no `GROSS PAY`, no `TAXABLE
    // EARNINGS`. Such a page cannot reconcile (it has deductions but no gross),
    // and its complete twin is elsewhere in the document, so it is skipped
    // rather than aborting the import. Absence of `GROSS PAY` is the signal.
    let mut saw_gross_pay = false;

    let mut block = Block::GrossAndAdjust;

    for l in lines.iter().skip(header_idx + 1) {
        if l.is_empty() {
            continue;
        }
        // NET PAY ends the CURRENT block (YTD numbers follow, descriptionless).
        if l.starts_with("NET PAY") {
            if let Some((_, amt)) = parse_line(l)? {
                net_pay = Some(amt);
            }
            break;
        }
        if l.starts_with("YEAR-TO-DATE") {
            break;
        }

        // Advance the block on the delimiting subtotals, then skip them.
        if l.starts_with("TAXABLE EARNINGS") {
            block = Block::Taxes;
            continue;
        }
        if l.starts_with("TAXES WITHHELD") {
            block = Block::AfterTax;
            continue;
        }
        if l.starts_with("GROSS PAY") {
            saw_gross_pay = true;
        }
        if is_control_line(l) {
            continue;
        }

        let Some((desc, amount)) = parse_line(l)? else {
            continue;
        };
        if amount.is_zero() {
            continue;
        }

        if amount.is_sign_positive() {
            // Income / imputed add-back.
            earnings.push(row(desc, amount));
        } else {
            // Deduction: store positive magnitude, routed by block.
            let mag = -amount;
            match block {
                Block::GrossAndAdjust => pre_tax_deductions.push(row(desc, mag)),
                Block::Taxes => employee_taxes.push(row(desc, mag)),
                Block::AfterTax => post_tax_deductions.push(row(desc, mag)),
            }
        }
    }

    let Some(net_pay) = net_pay else {
        return Ok(None);
    };
    // Defective reprint: CURRENT block had deductions but no earnings section
    // (see `saw_gross_pay`). Skip it; the complete twin is parsed elsewhere.
    if !saw_gross_pay {
        return Ok(None);
    }
    let check_date = check_date.ok_or_else(|| anyhow!("page {page_num}: Check Date not found"))?;
    let period_begin = period_begin.unwrap_or(check_date);
    let period_end = period_end.unwrap_or(check_date);

    // Reconcile: earnings − (all deductions) == net pay.
    let total_deductions = sum_amounts(&employee_taxes)
        + sum_amounts(&pre_tax_deductions)
        + sum_amounts(&post_tax_deductions);
    let reconstructed = sum_amounts(&earnings) - total_deductions;
    if (reconstructed - net_pay).abs() > Decimal::new(1, 2) {
        return Err(anyhow!(
            "page {page_num}: reconstructed net {reconstructed} != statement Net Pay {net_pay} \
             (diff {})",
            reconstructed - net_pay
        ));
    }

    Ok(Some(ParsedPage {
        page_num,
        check_date,
        period_begin,
        period_end,
        net_pay,
        earnings,
        employee_taxes,
        pre_tax_deductions,
        post_tax_deductions,
    }))
}

/// Parse every statement page of an Official-Copy Microsoft PDF into
/// [`ParsedPage`]s. This is the backend entry point dispatched to from
/// [`super::parse_pdf`].
pub fn parse_pdf(pdf_path: &std::path::Path) -> Result<Vec<ParsedPage>> {
    let pages = pdf_extract::extract_text_by_pages(pdf_path)
        .context("Failed to extract text from Microsoft PDF")?;

    let mut parsed = Vec::new();
    for (i, page_text) in pages.iter().enumerate() {
        if let Some(page) = parse_page(page_text, i + 1)? {
            parsed.push(page);
        }
    }
    if parsed.is_empty() {
        anyhow::bail!("no importable statement pages found in Microsoft PDF");
    }
    Ok(parsed)
}
