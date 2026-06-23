use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use jiff::civil::Date;
use regex::Regex;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::str::FromStr;
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

#[derive(Debug, Clone, Default)]
#[expect(dead_code)]
pub struct RowData {
    pub description: String,
    pub dates: String,
    pub values: HashMap<String, Decimal>,
}

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

#[derive(Debug, PartialEq, Eq)]
enum ParseState {
    None,
    Earnings,
    EmployeeTaxes,
    PreTaxDeductions,
    PostTaxDeductions,
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

pub fn convert_pdf_to_pages(pdf_path: &std::path::Path) -> Result<Vec<String>> {
    pdf_extract::extract_text_by_pages(pdf_path).context("Failed to extract text from PDF")
}
