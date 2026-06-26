//! ADP-generated Microsoft Corporation earnings-statement backend.
//!
//! These are the statements ADP produces for Microsoft payroll (footer
//! `AutomaticData Processing (PCSUVO)`), distinct from the in-house "Official
//! Copy of Microsoft Corporation Earnings Statement" handled by [`super::microsoft`].
//!
//! ## Why this backend is non-trivial
//!
//! The statement is a two-physical-column page: the left column carries the
//! `Earnings` / `Taxes` / `Benefits` / `Other` tables, the right column carries
//! `Retirement` (the 401(k) line), the `Net Pay` line, and a block of
//! employer-paid / informational figures. `pdf-extract`'s text extraction
//! *interleaves* the two columns line-by-line, so a naive top-to-bottom read is
//! scrambled. The one reliable signal the extractor preserves is indentation:
//! **left-column lines start at column 0; right-column lines are emitted with a
//! leading space.** This backend deinterleaves on that signal, then runs a
//! small section state machine over each column.
//!
//! ## Sign convention
//!
//! ADP renders credits/deductions with a *trailing* minus (`622.96-`), which
//! [`super::clean_decimal`] already normalises. The importer expects each
//! section stored in a fixed sign convention (earnings positive — it negates
//! them; taxes / pre-tax / post-tax positive-as-debit). Because ADP's natural
//! signs are the mirror of that for every non-earning section, the rule is
//! uniform: **earnings are stored with their ADP sign; every other section is
//! stored negated.** This makes the four sections reconcile to net pay exactly,
//! which [`parse_pdf`] asserts against each `Total <Section>` checksum line and
//! against the `Net Pay` figure before returning a page.
//!
//! The leading `*` ADP puts on `*401K Pre Tax` (its "excluded from federal
//! taxable wages" marker) is stripped: although the importer only injects
//! imputed-income offsets for providers that need it (Workday — ADP-Microsoft
//! reconciles on its own), stripping the marker keeps the parsed descriptions
//! clean and avoids any reliance on backend dispatch to suppress it. ADP already
//! states stock/disability offsets explicitly in the `Other` section, so no
//! synthetic offset is wanted.

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

/// A money token: optional thousands-separated integer part, a 2+ digit
/// fractional part, optional trailing minus. Bare integers (hours, ids) are
/// matched separately where a row's numeric tail is collected.
static MONEY_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^-?[\d,]+\.\d{2,}-?$").unwrap());

/// A bare integer token (e.g. an `Hours` column), thousands-separators allowed.
static INT_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^-?[\d,]+-?$").unwrap());

/// `MM/DD/YYYY` anywhere in a line.
static DATE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b\d{2}/\d{2}/\d{4}\b").unwrap());

fn is_numeric_token(tok: &str) -> bool {
    MONEY_RE.is_match(tok) || INT_RE.is_match(tok)
}

/// Left-column numeric width of the `Earnings` table: `Rate Hours ThisPeriod
/// YTD` (Rate/Hours may be absent on some rows, but never more than four).
const EARN_COLS: usize = 4;
/// Left-column numeric width of every other section: `ThisPeriod YTD`.
const TWO_COLS: usize = 2;

/// Split a data row into `(description, this_period_amount)`.
///
/// ADP prints each left-column row as `<description> [Rate] [Hours] ThisPeriod
/// YTD`. `pdf-extract` sometimes appends right-column content (a wrapped
/// "Federal taxable wages …" label, or a stray right-column number) onto the
/// same physical line, so the reliable signal is the *leading* contiguous run
/// of numeric tokens immediately after the description — appended right-column
/// *text* terminates that run on its own, and appended right-column *numbers*
/// are dropped by capping the run to `max_cols`, the section's real column
/// count.
///
/// Within the (capped) left run the current-period figure is the
/// second-to-last token and the YTD is the last; a single-token run is a
/// YTD-only line (current period blank) and yields `0.00`. Returns `None` for a
/// line with no numeric tail (a header or pure-text line).
fn parse_row(line: &str, max_cols: usize) -> Result<Option<RowData>> {
    let line = line.trim();
    if line.is_empty() {
        return Ok(None);
    }

    let tokens: Vec<&str> = line.split_whitespace().collect();

    // Description = leading tokens up to the first numeric token.
    let Some(first_num) = tokens.iter().position(|t| is_numeric_token(t)) else {
        return Ok(None);
    };
    let description = tokens[..first_num]
        .join(" ")
        .trim_start_matches('*')
        .trim()
        .to_string();
    if description.is_empty() {
        return Ok(None);
    }

    // Leading contiguous numeric run after the description, capped to the
    // section's real column count to shed appended right-column numbers.
    let mut run_end = first_num;
    while run_end < tokens.len() && is_numeric_token(tokens[run_end]) {
        run_end += 1;
    }
    let mut run: &[&str] = &tokens[first_num..run_end];
    if run.len() > max_cols {
        run = &run[..max_cols];
    }
    if run.is_empty() {
        return Ok(None);
    }

    // Within the (capped) run the current-period figure is the second-to-last
    // token and YTD is the last; a single-token run is YTD-only (current period
    // blank → 0). One failure mode survives the cap: when the current-period
    // column is blank, `pdf-extract` can append a stray right-column number (an
    // interleaved "Federal taxable wages for the period:" value) into the last
    // slot, leaving the row as `[YTD, right-col-junk]` — indistinguishable from
    // a genuine `[ThisPeriod, YTD]` pair by token count alone (this is what
    // scrambles the stock-vest `Stock Award …Offset` lines). The reliable
    // signal is that YTD accumulates the current period, so |ThisPeriod| <=
    // |YTD| holds for any real pair. If the second-to-last token's magnitude
    // exceeds the last's, the current-period column was blank and the apparent
    // this-period token is really the YTD shifted left by appended junk → the
    // current period is 0. A genuine pair that violated this (only possible
    // with a same-year reversal) would instead trip the per-section `Total`
    // checksum in `parse_page` and refuse the import — never corrupt it.
    let amount = if run.len() >= 2 {
        let this_period =
            clean_decimal(run[run.len() - 2]).with_context(|| format!("adp row {line:?}"))?;
        let ytd = clean_decimal(run[run.len() - 1]).with_context(|| format!("adp row {line:?}"))?;
        if this_period.abs() > ytd.abs() {
            Decimal::ZERO
        } else {
            this_period
        }
    } else {
        Decimal::ZERO
    };

    let mut values = HashMap::new();
    values.insert("Amount".to_string(), amount);
    Ok(Some(RowData {
        description,
        dates: String::new(),
        values,
    }))
}

/// The current-period figure from a `Total <Section> … <YTD>` line, used as a
/// per-section checksum. Shares [`parse_row`]'s leading-run logic so an appended
/// right-column label or number does not corrupt the total. Returns `None` if
/// the line has no numeric tail.
fn total_this_period(line: &str, max_cols: usize) -> Result<Option<Decimal>> {
    Ok(parse_row(line, max_cols)?.map(|r| r.amount()))
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum Section {
    None,
    Earnings,
    Taxes,
    Benefits,
    Other,
    Retirement,
}

/// Header metadata scraped from a page: the period and advice (check) dates.
struct Header {
    check_date: Date,
    period_begin: Date,
    period_end: Date,
}

/// Parse the `Period Beg/End:` / `Advice Date:` header band. These labels can
/// land in either deinterleaved column, so the whole page text is scanned. The
/// period begin date sits on the `Period Beg/End:` line; the end date is the
/// next `MM/DD/YYYY` to appear. The advice date is the check date.
fn parse_header(page: &str) -> Result<Option<Header>> {
    let lines: Vec<&str> = page.lines().collect();

    let mut period_begin: Option<Date> = None;
    let mut period_end: Option<Date> = None;
    let mut check_date: Option<Date> = None;

    for (i, line) in lines.iter().enumerate() {
        if period_begin.is_none() && line.contains("Period Beg/End:") {
            if let Some(m) = DATE_RE.find(line) {
                period_begin = Some(parse_date_str(m.as_str())?);
                // End date is the next date token, on this line or a following one.
                let after = &line[m.end()..];
                if let Some(m2) = DATE_RE.find(after) {
                    period_end = Some(parse_date_str(m2.as_str())?);
                } else {
                    for nl in lines.iter().skip(i + 1).take(4) {
                        if let Some(m2) = DATE_RE.find(nl) {
                            period_end = Some(parse_date_str(m2.as_str())?);
                            break;
                        }
                    }
                }
            }
        }
        if check_date.is_none() && line.contains("Advice Date:") {
            if let Some(m) = DATE_RE.find(line) {
                check_date = Some(parse_date_str(m.as_str())?);
            }
        }
    }

    let Some(check_date) = check_date else {
        return Ok(None);
    };
    let period_begin = period_begin.unwrap_or(check_date);
    let period_end = period_end.unwrap_or(check_date);
    Ok(Some(Header {
        check_date,
        period_begin,
        period_end,
    }))
}

/// Classify a left-column line as a section header (returns the new section) or
/// `None` if it is not a header.
fn left_section_header(line: &str) -> Option<Section> {
    let t = line.trim();
    if t.starts_with("Earnings") {
        Some(Section::Earnings)
    } else if t == "Taxes" {
        Some(Section::Taxes)
    } else if t == "Benefits" {
        Some(Section::Benefits)
    } else if t == "Other" {
        Some(Section::Other)
    } else {
        None
    }
}

/// Negate every stored amount in a section, in place. Used to flip ADP's
/// natural sign for the non-earning sections into the importer's convention.
fn negate(rows: &mut [RowData]) {
    for r in rows.iter_mut() {
        if let Some(a) = r.values.get_mut("Amount") {
            *a = -*a;
        }
    }
}

fn sum_amounts(rows: &[RowData]) -> Decimal {
    rows.iter().map(|r| r.amount()).sum()
}

/// Parse one page into a [`ParsedPage`], or `None` if the page is a
/// continuation/explanatory page with no `Net Pay` line (e.g. the "earnings
/// from prior pay periods" page that accompanies a stock-vest statement).
fn parse_page(page: &str, page_num: usize) -> Result<Option<ParsedPage>> {
    let page = page.replace('\u{0}', "");
    let lines: Vec<&str> = page.lines().collect();

    let mut earnings: Vec<RowData> = Vec::new();
    let mut taxes: Vec<RowData> = Vec::new();
    let mut benefits: Vec<RowData> = Vec::new();
    let mut other: Vec<RowData> = Vec::new();
    let mut retirement: Vec<RowData> = Vec::new();

    // Per-section checksum targets read from the `Total <Section>` lines.
    let mut tot_earn: Option<Decimal> = None;
    let mut tot_tax: Option<Decimal> = None;
    let mut tot_ben: Option<Decimal> = None;
    let mut tot_oth: Option<Decimal> = None;

    let mut net_pay: Option<Decimal> = None;

    let mut left = Section::None;
    let mut right = Section::None;

    for raw in &lines {
        if raw.trim().is_empty() {
            continue;
        }
        // Deinterleave: right-column lines are emitted with leading whitespace.
        let is_right = raw.starts_with(' ') || raw.starts_with('\t');
        let line = raw.trim();

        if is_right {
            // Right column: Retirement section + the Net Pay line.
            if line.starts_with("Net Pay Distribution") {
                right = Section::None;
                continue;
            }
            if line.starts_with("Net Pay") {
                if let Some(row) = parse_row(line, TWO_COLS)? {
                    net_pay = Some(row.amount());
                }
                right = Section::None;
                continue;
            }
            if line == "Retirement" {
                right = Section::Retirement;
                continue;
            }
            if line.starts_with("Total Retirement") {
                right = Section::None;
                continue;
            }
            if line.starts_with("Other Benefits")
                || line.starts_with("Personal Time")
                || line.starts_with("Federal taxable")
            {
                right = Section::None;
                continue;
            }
            if right == Section::Retirement
                && let Some(row) = parse_row(line, TWO_COLS)?
            {
                retirement.push(row);
            }
            continue;
        }

        // Left column.
        if let Some(hdr) = left_section_header(line) {
            left = hdr;
            continue;
        }
        if line.starts_with("Total ") {
            match left {
                Section::Earnings => tot_earn = total_this_period(line, EARN_COLS)?,
                Section::Taxes => tot_tax = total_this_period(line, TWO_COLS)?,
                Section::Benefits => tot_ben = total_this_period(line, TWO_COLS)?,
                Section::Other => tot_oth = total_this_period(line, TWO_COLS)?,
                _ => {}
            }
            left = Section::None;
            continue;
        }
        if line.starts_with("Federal taxable")
            || line.starts_with("Taxable Wages")
            || line.starts_with("TAXABLE")
        {
            left = Section::None;
            continue;
        }

        match left {
            Section::Earnings => {
                if let Some(row) = parse_row(line, EARN_COLS)? {
                    earnings.push(row);
                }
            }
            Section::Taxes => {
                if let Some(row) = parse_row(line, TWO_COLS)? {
                    taxes.push(row);
                }
            }
            Section::Benefits => {
                if let Some(row) = parse_row(line, TWO_COLS)? {
                    benefits.push(row);
                }
            }
            Section::Other => {
                if let Some(row) = parse_row(line, TWO_COLS)? {
                    other.push(row);
                }
            }
            _ => {}
        }
    }

    // No Net Pay line → continuation/explanatory page, nothing to import.
    let Some(net_pay) = net_pay else {
        return Ok(None);
    };

    let header = parse_header(&page)?
        .ok_or_else(|| anyhow!("page {page_num}: ADP header (Advice Date) not found"))?;

    // Verify each section sum against its printed Total before flipping signs.
    let check = |name: &str, rows: &[RowData], total: Option<Decimal>| -> Result<()> {
        if let Some(t) = total {
            let s = sum_amounts(rows);
            if (s - t).abs() > Decimal::new(1, 2) {
                return Err(anyhow!(
                    "page {page_num}: {name} rows sum to {s} but statement Total is {t} \
                     (column extraction is misaligned)"
                ));
            }
        }
        Ok(())
    };
    check("Earnings", &earnings, tot_earn)?;
    check("Taxes", &taxes, tot_tax)?;
    check("Benefits", &benefits, tot_ben)?;
    check("Other", &other, tot_oth)?;

    // Master reconciliation in ADP's natural signs:
    //   net = earnings + taxes + benefits + other + retirement
    let natural = sum_amounts(&earnings)
        + sum_amounts(&taxes)
        + sum_amounts(&benefits)
        + sum_amounts(&other)
        + sum_amounts(&retirement);
    if (natural - net_pay).abs() > Decimal::new(1, 2) {
        return Err(anyhow!(
            "page {page_num}: sections reconcile to {natural} but Net Pay is {net_pay} \
             (diff {})",
            natural - net_pay
        ));
    }

    // Flip every non-earning section into the importer's stored-sign convention.
    negate(&mut taxes);
    negate(&mut benefits);
    negate(&mut other);
    negate(&mut retirement);

    // `Other` (stock/disability offsets) and `Benefits` are both post-tax in
    // the importer's model; `Retirement` is the pre-tax 401(k).
    let mut post_tax_deductions = benefits;
    post_tax_deductions.extend(other);

    Ok(Some(ParsedPage {
        page_num,
        check_date: header.check_date,
        period_begin: header.period_begin,
        period_end: header.period_end,
        net_pay,
        earnings,
        employee_taxes: taxes,
        pre_tax_deductions: retirement,
        post_tax_deductions,
    }))
}

/// Parse every statement page of an ADP-Microsoft PDF into [`ParsedPage`]s.
/// Continuation pages (no `Net Pay` line) are skipped. This is the backend
/// entry point dispatched to from [`super::parse_pdf`].
pub fn parse_pdf(pdf_path: &std::path::Path) -> Result<Vec<ParsedPage>> {
    let pages = pdf_extract::extract_text_by_pages(pdf_path)
        .context("Failed to extract text from ADP Microsoft PDF")?;

    let mut parsed = Vec::new();
    for (i, page_text) in pages.iter().enumerate() {
        if let Some(page) = parse_page(page_text, i + 1)? {
            parsed.push(page);
        }
    }
    if parsed.is_empty() {
        anyhow::bail!("no importable statement pages found in ADP Microsoft PDF");
    }
    Ok(parsed)
}
