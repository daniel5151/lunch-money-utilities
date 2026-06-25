//! Workday payslip backend (e.g. Meta).
//!
//! Workday PDFs extract cleanly as plain text with `pdf-extract`'s
//! `extract_text_by_pages`, so this backend works line-by-line: a small state
//! machine walks the section headers ("Earnings", "Employee Taxes", "Pre Tax
//! Deductions", "Post Tax Deductions") and parses each row from its tokens.

use super::ParsedPage;
use super::RowData;
use super::clean_decimal;
use super::parse_date_str;
use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use jiff::civil::Date;
use regex::Regex;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::sync::LazyLock;

/// `pdf-extract` frequently glues an earnings row's period *start date* onto the
/// final description token (e.g. `Restricted Stock Units11/15/2025`) with no
/// separating space. Left as-is, the glued token still contains `/`, which trips
/// the date-range detection in [`parse_earnings_line`] and silently truncates the
/// real last word off the description. Splitting the date back out before
/// tokenizing repairs the description without relying on brittle positional
/// assumptions.
static GLUED_DATE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"([^\s/])(\d{2}/\d{2}/\d{4})").unwrap());

/// Insert a space between a non-whitespace, non-`/` character and a glued
/// `MM/DD/YYYY` date so the date becomes its own token. Safe to run on every
/// line: rows that already separate the date are left unchanged.
fn deglue_dates(line: &str) -> std::borrow::Cow<'_, str> {
    GLUED_DATE_RE.replace_all(line, "$1 $2")
}

/// Whether a Workday line description denotes imputed income — a taxable item
/// added back into gross pay but not paid in cash, which the importer must
/// offset for the paycheck to reconcile to net pay.
///
/// Workday marks most imputed lines with a leading `*` (`*Imp GTL` =
/// imputed group-term life; `*Relo Qualified` = qualified relocation). A few
/// imputed lines carry no marker (`Relocation Tax Ben`, the tax-benefit
/// gross-up companion); those are listed exactly (case-insensitive, trimmed) in
/// `extra` via per-backend config. Matching is exact, not substring, so a short
/// entry cannot capture an unrelated sibling line (e.g. `Relocation Tax Ben`
/// does not also match `Relocation Tax Ben Adjustment`).
pub fn is_imputed_income(description: &str, extra: &[String]) -> bool {
    let desc = description.trim();
    if desc.starts_with('*') {
        return true;
    }
    extra
        .iter()
        .any(|entry| desc.eq_ignore_ascii_case(entry.trim()))
}

#[derive(Debug, PartialEq, Eq)]
enum ParseState {
    None,
    Earnings,
    EmployeeTaxes,
    PreTaxDeductions,
    PostTaxDeductions,
}

pub fn parse_earnings_line(line: &str) -> Result<Option<RowData>> {
    let deglued = deglue_dates(line);
    let line = deglued.trim();
    if line.is_empty()
        || line.starts_with("Description")
        || line.starts_with("Total")
        || line.starts_with("Earnings")
    {
        return Ok(None);
    }

    let tokens: Vec<&str> = line.split_whitespace().collect();
    if tokens.len() < 5 {
        return Ok(None);
    }

    let amount_str = tokens[tokens.len() - 3];
    let amount = clean_decimal(amount_str).with_context(|| format!("earnings line {line:?}"))?;

    let mut date_str = String::new();
    let mut desc_end = tokens.len() - 5;

    if tokens.len() >= 8 {
        let t_mid = tokens[tokens.len() - 7];
        if t_mid == "-" {
            let t_start = tokens[tokens.len() - 8];
            let t_end = tokens[tokens.len() - 6];
            if t_start.contains('/') && t_end.contains('/') {
                date_str = format!("{} - {}", t_start, t_end);
                desc_end = tokens.len() - 8;
            }
        }
    }

    let description = tokens[..desc_end].join(" ");

    let mut values = HashMap::new();
    values.insert("Amount".to_string(), amount);

    Ok(Some(RowData {
        description,
        dates: date_str,
        values,
    }))
}

pub fn parse_deduction_tax_line(line: &str) -> Result<Option<RowData>> {
    let line = line.trim();
    if line.is_empty()
        || line.starts_with("Description")
        || line.starts_with("Total")
        || line.starts_with("Pre Tax")
        || line.starts_with("Post Tax")
        || line.starts_with("Employee Taxes")
        || line.starts_with("Earnings")
    {
        return Ok(None);
    }

    let tokens: Vec<&str> = line.split_whitespace().collect();
    if tokens.len() < 2 {
        return Ok(None);
    }

    let last_is_num = tokens
        .last()
        .map(|&t| {
            t.replace(',', "")
                .trim_end_matches('-')
                .parse::<f64>()
                .is_ok()
        })
        .unwrap_or(false);
    let second_last_is_num = if tokens.len() >= 3 {
        tokens
            .get(tokens.len() - 2)
            .map(|&t| {
                t.replace(',', "")
                    .trim_end_matches('-')
                    .parse::<f64>()
                    .is_ok()
            })
            .unwrap_or(false)
    } else {
        false
    };

    let (description, amount) = if second_last_is_num && last_is_num {
        let amount_str = tokens[tokens.len() - 2];
        let desc = tokens[..tokens.len() - 2].join(" ");
        (
            desc,
            clean_decimal(amount_str).with_context(|| format!("deduction/tax line {line:?}"))?,
        )
    } else if last_is_num {
        let desc = tokens[..tokens.len() - 1].join(" ");
        (desc, Decimal::ZERO)
    } else {
        return Ok(None);
    };

    let mut values = HashMap::new();
    values.insert("Amount".to_string(), amount);

    Ok(Some(RowData {
        description,
        dates: String::new(),
        values,
    }))
}

pub fn parse_page_tables(page_text: &str, page_num: usize) -> Result<ParsedPage> {
    let lines: Vec<&str> = page_text.lines().collect();
    let date_re = Regex::new(r"\b\d{2}/\d{2}/\d{4}\b").unwrap();

    let mut check_date: Option<Date> = None;
    let mut period_begin: Option<Date> = None;
    let mut period_end: Option<Date> = None;

    for (line_idx, line) in lines.iter().enumerate() {
        if line.contains("Pay Period Begin") && line.contains("Check Date") {
            for next_line in lines.iter().skip(line_idx + 1).take(3) {
                let dates: Vec<&str> = date_re.find_iter(next_line).map(|m| m.as_str()).collect();
                if dates.len() >= 3 {
                    period_begin = Some(parse_date_str(dates[0])?);
                    period_end = Some(parse_date_str(dates[1])?);
                    check_date = Some(parse_date_str(dates[2])?);
                    break;
                }
            }
            if check_date.is_some() {
                break;
            }
        }
    }

    if check_date.is_none() {
        let dates: Vec<&str> = date_re.find_iter(page_text).map(|m| m.as_str()).collect();
        if dates.len() >= 3 {
            period_begin = Some(parse_date_str(dates[0])?);
            period_end = Some(parse_date_str(dates[1])?);
            check_date = Some(parse_date_str(dates[2])?);
        }
    }

    let check_date =
        check_date.ok_or_else(|| anyhow!("Page {}: check date not found", page_num))?;
    let period_begin = period_begin.unwrap_or(check_date);
    let period_end = period_end.unwrap_or(check_date);

    let mut net_pay = Decimal::ZERO;
    for line in &lines {
        let line_strip = line.trim();
        if line_strip.starts_with("Current") && !line_strip.contains("Hours Worked") {
            let parts: Vec<&str> = line_strip.split_whitespace().collect();
            if let Some(last_part) = parts.last() {
                net_pay = clean_decimal(last_part)
                    .with_context(|| format!("page {page_num}: net pay line {line_strip:?}"))?;
            }
        }
    }

    let mut earnings = Vec::new();
    let mut employee_taxes = Vec::new();
    let mut pre_tax_deductions = Vec::new();
    let mut post_tax_deductions = Vec::new();

    let mut state = ParseState::None;

    for line in &lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if trimmed == "Earnings" {
            state = ParseState::Earnings;
            continue;
        } else if trimmed == "Employee Taxes" {
            state = ParseState::EmployeeTaxes;
            continue;
        } else if trimmed == "Pre Tax Deductions" {
            state = ParseState::PreTaxDeductions;
            continue;
        } else if trimmed == "Post Tax Deductions" {
            state = ParseState::PostTaxDeductions;
            continue;
        } else if trimmed.starts_with("Taxable Wages")
            || trimmed.starts_with("Federal State Absence Plans")
            || trimmed.starts_with("Payment Information")
            || trimmed.starts_with("Employer Paid")
        {
            state = ParseState::None;
            continue;
        }

        match state {
            ParseState::Earnings => {
                if let Some(row) = parse_earnings_line(line)? {
                    earnings.push(row);
                }
            }
            ParseState::EmployeeTaxes => {
                if let Some(row) = parse_deduction_tax_line(line)? {
                    employee_taxes.push(row);
                }
            }
            ParseState::PreTaxDeductions => {
                if let Some(row) = parse_deduction_tax_line(line)? {
                    pre_tax_deductions.push(row);
                }
            }
            ParseState::PostTaxDeductions => {
                if let Some(row) = parse_deduction_tax_line(line)? {
                    post_tax_deductions.push(row);
                }
            }
            ParseState::None => {}
        }
    }

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

/// Parse every page of a Workday payslip PDF into [`ParsedPage`]s. Empty pages
/// (blank trailing pages, separators) are skipped. This is the backend entry
/// point dispatched to from [`super::parse_pdf`].
pub fn parse_pdf(pdf_path: &std::path::Path) -> Result<Vec<ParsedPage>> {
    let pages = pdf_extract::extract_text_by_pages(pdf_path)
        .context("Failed to extract text from PDF")?;

    let mut parsed = Vec::new();
    for (i, page_text) in pages.iter().enumerate() {
        let page_num = i + 1;
        let page_text = page_text.trim();
        if page_text.is_empty() {
            continue;
        }
        parsed.push(parse_page_tables(page_text, page_num)?);
    }
    Ok(parsed)
}


#[cfg(test)]
mod tests {
    use super::is_imputed_income;

    #[test]
    fn starred_descriptions_are_always_imputed() {
        assert!(is_imputed_income("*Imp GTL", &[]));
        assert!(is_imputed_income("*Relo Qualified", &[]));
        // Leading/trailing whitespace around the marker is tolerated.
        assert!(is_imputed_income("  *Imp GTL  ", &[]));
    }

    #[test]
    fn unmarked_descriptions_are_imputed_only_when_listed() {
        let extra = vec!["Relocation Tax Ben".to_string()];
        assert!(is_imputed_income("Relocation Tax Ben", &extra));
        // Case-insensitive, whitespace-trimmed.
        assert!(is_imputed_income("relocation tax ben", &extra));
        assert!(is_imputed_income("  Relocation Tax Ben ", &extra));
        // Not listed -> not imputed.
        assert!(!is_imputed_income("Relocation Tax Ben", &[]));
        assert!(!is_imputed_income("Salary", &extra));
    }

    #[test]
    fn matching_is_exact_not_substring() {
        let extra = vec!["Relocation Tax Ben".to_string()];
        // A longer sibling line must not be captured by a shorter entry.
        assert!(!is_imputed_income("Relocation Tax Ben Adjustment", &extra));
        // Nor a shorter prefix.
        assert!(!is_imputed_income("Relocation Tax", &extra));
    }
}
