use crate::style::*;
use anstream::eprintln;
use anstream::println;
use anyhow::Context;
use lunch_money::client::Client as LunchMoneyClient;
use lunch_money::client::TooManyRequestsPolicy;
use std::fs;

#[derive(Clone)]
struct LunchMoneyAccount {
    name: String,
    display_name: Option<String>,
}

impl std::fmt::Display for LunchMoneyAccount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(ref disp) = self.display_name {
            if disp != &self.name {
                return write!(f, "{} ({})", disp, self.name);
            }
            write!(f, "{}", disp)
        } else {
            write!(f, "{}", self.name)
        }
    }
}

pub(crate) async fn run_init(args: crate::cli::InitArgs) -> anyhow::Result<()> {
    if args.pdfs.is_empty() {
        println! {};
        println! { "{STYLE_WARNING}⚠️  Warning: No payslip PDF files were provided to seed category mappings.{STYLE_WARNING:#}" };
        println! { "It is strongly recommended to run the wizard with one or more payslip PDFs to automatically" };
        println! { "generate the list of unique payslip items that need to be mapped to Lunch Money categories." };
        println! {};
        println! { "💡 Tip: The more payslip data provided, the more comprehensive the generated mappings list will be." };
        println! { "   Downloading a single combined multi-page PDF of your payslips directly from your provider (e.g., Workday) is ideal." };
        println! { "   Alternatively, you can pass multiple individual payslip PDF files." };
        println! {};
        let proceed =
            inquire::Confirm::new("Are you sure you want to continue without any seeding PDFs?")
                .with_default(false)
                .prompt()
                .context("Failed to get continuation choice")?;
        if !proceed {
            println! {};
            println! { "{STYLE_INFO}Aborted. Please run the command with path(s) to payslip PDF file(s):{STYLE_INFO:#}" };
            println! { "  {STYLE_HEADER}lm-payslip-importer init [PDF_PATHS...]{STYLE_HEADER:#}" };
            println! {};
            return Ok(());
        }
    }

    let output_path = args
        .file
        .clone()
        .unwrap_or_else(|| std::path::PathBuf::from("lm_payslip_importer.toml"));

    if output_path.exists() {
        anyhow::bail!(
            "{} already exists in this directory.",
            output_path.display()
        );
    }

    println! {};
    println! { "{STYLE_HEADER}⚙️  Configuring Lunch Money Payslip Importer{STYLE_HEADER:#}" };
    println! { "{STYLE_DIM}─────────────────────────────────────────────────────────────────{STYLE_DIM:#}" };
    println! { "{STYLE_INFO}This wizard will help you set up {}.{STYLE_INFO:#}", output_path.display() };
    println! {};

    let lunch_money_api_key = inquire::Password::new("Lunch Money API Key:")
        .with_help_message("Your Lunch Money developer API key")
        .with_display_mode(inquire::PasswordDisplayMode::Masked)
        .without_confirmation()
        .prompt()
        .context("Failed to get Lunch Money API Key")?;

    println! {};
    println! { "{STYLE_INFO}🔗 Connecting to Lunch Money API...{STYLE_INFO:#}" };

    let http_client = reqwest::Client::new();
    let lm_client = LunchMoneyClient::new(
        http_client.clone(),
        lunch_money_api_key.clone(),
        TooManyRequestsPolicy::Retry {
            max_retries: 5,
            initial_delay: std::time::Duration::from_secs(2),
        },
    );

    let plaid_accts = lm_client
        .fetch_plaid_accounts()
        .await
        .context("Failed to fetch Plaid accounts")?;
    let manual_accts = lm_client
        .fetch_manual_accounts()
        .await
        .context("Failed to fetch manual accounts")?;

    let cat_query = lunch_money::categories::query_params::CategoryQuery::builder()
        .format("flattened".to_string())
        .build();
    let lm_categories = lm_client
        .fetch_categories(&cat_query)
        .await
        .context("Failed to fetch Lunch Money categories")?;

    let lm_tags = lm_client
        .fetch_tags()
        .await
        .context("Failed to fetch Lunch Money tags")?;

    let mut accounts = Vec::new();
    for acct in plaid_accts {
        if acct.status == lunch_money::plaid_accounts::schemas::PlaidAccountStatus::Active {
            accounts.push(LunchMoneyAccount {
                name: acct.name,
                display_name: acct.display_name,
            });
        }
    }
    for acct in manual_accts {
        if acct.status == lunch_money::manual_accounts::schemas::AccountStatus::Active {
            accounts.push(LunchMoneyAccount {
                name: acct.name,
                display_name: acct.display_name,
            });
        }
    }

    if accounts.is_empty() {
        anyhow::bail!("No active Plaid or manual accounts found in your Lunch Money account.");
    }

    accounts.sort_by(|a, b| {
        let a_disp = a.display_name.as_deref().unwrap_or(&a.name);
        let b_disp = b.display_name.as_deref().unwrap_or(&b.name);
        a_disp.cmp(b_disp)
    });

    println! {};
    let use_tag = inquire::Confirm::new("Would you like to set an optional tag for transactions created by this importer?")
        .with_default(false)
        .prompt()
        .context("Failed to get tag preference")?;

    let tag_name = if use_tag {
        let mut active_tags: Vec<String> = lm_tags
            .iter()
            .filter(|t| !t.archived)
            .map(|t| t.name.clone())
            .collect();
        active_tags.sort();

        let choice = if active_tags.is_empty() {
            println! { "No active tags found in your Lunch Money account." };
            inquire::Text::new("Enter new tag name to create:")
                .prompt()
                .context("Failed to get new tag name")?
        } else {
            let mut options = active_tags;
            options.push("<Create new tag / Custom tag...>".to_string());
            let select_choice = inquire::Select::new("Select Tag:", options)
                .prompt()
                .context("Failed to select tag")?;

            if select_choice == "<Create new tag / Custom tag...>" {
                inquire::Text::new("Enter tag name:")
                    .prompt()
                    .context("Failed to get custom tag name")?
            } else {
                select_choice
            }
        };

        let trimmed = choice.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    } else {
        None
    };

    let tag_line = if let Some(ref name) = tag_name {
        format!(
            "# Optional tag to use on transactions created by this importer\ntag = \"{}\"",
            name
        )
    } else {
        "# Optional tag to use on transactions created by this importer\n# tag = \"\"".to_string()
    };

    // Seed the per-provider mapping tables. Each seed PDF is fingerprinted to
    // its provider so a single `init` invocation can configure several backends
    // at once (e.g. a Workday PDF and a Microsoft PDF), and each provider's
    // unique line items land in that provider's own [backends.<kind>.mapping].
    use crate::payslip::PayslipKind;
    let mut per_backend_items: std::collections::BTreeMap<
        PayslipKind,
        std::collections::BTreeSet<String>,
    > = std::collections::BTreeMap::new();

    if !args.pdfs.is_empty() {
        for pdf_path in &args.pdfs {
            println! {};
            println! { "{STYLE_INFO}📄 Parsing payslip PDF to seed mappings: {}{STYLE_INFO:#}", pdf_path.display() };
            // Detect which payroll provider produced this PDF so seeding works
            // for any supported backend, not just Workday. Fall back to Workday
            // if the fingerprint is inconclusive (the historical default).
            let kind = match crate::payslip::detect_kind(pdf_path) {
                Ok(Some(k)) => k,
                Ok(None) => PayslipKind::Workday,
                Err(e) => {
                    eprintln! { "{STYLE_ERROR}❌ Failed to detect payslip provider for {}: {}{STYLE_ERROR:#}", pdf_path.display(), e };
                    continue;
                }
            };
            println! { "  Detected payslip provider: {kind}" };
            let pages = match crate::payslip::parse_pdf(pdf_path, kind) {
                Ok(p) => p,
                Err(e) => {
                    eprintln! { "{STYLE_ERROR}❌ Failed to parse {}: {}{STYLE_ERROR:#}", pdf_path.display(), e };
                    continue;
                }
            };
            let bucket = per_backend_items.entry(kind).or_default();
            for parsed in pages {
                // Only seed items that actually appeared with a non-zero
                // *current* amount. YTD-only rows (zero current) would
                // otherwise pollute the mapping list with items the user
                // never needs to categorize (audit #10).
                let mut collect = |rows: Vec<crate::payslip::RowData>| {
                    for item in rows {
                        if item.amount().is_zero() {
                            continue;
                        }
                        let desc = item.description.trim().to_string();
                        if !desc.is_empty() {
                            bucket.insert(desc);
                        }
                    }
                };
                collect(parsed.earnings);
                collect(parsed.employee_taxes);
                collect(parsed.pre_tax_deductions);
                collect(parsed.post_tax_deductions);
            }
        }
    }

    // With no seed PDFs we cannot know the provider, so scaffold a single
    // Workday backend (the historical default) with commented example mappings.
    let backends_to_configure: Vec<PayslipKind> = if per_backend_items.is_empty() {
        vec![PayslipKind::Workday]
    } else {
        per_backend_items.keys().copied().collect()
    };

    // Prompt for the provider-specific settings (deposit account, payee, and —
    // for providers that reconstruct RSU vests — the RSU account + vest payee)
    // once per detected backend, then assemble that backend's TOML section.
    let mut backend_sections = String::new();
    for kind in &backends_to_configure {
        let kind = *kind;
        println! {};
        println! { "{STYLE_HEADER}⚙️  Configuring backend: {kind}{STYLE_HEADER:#}" };

        let selected_net_zero =
            inquire::Select::new("Select Net Zero Account:", accounts.clone())
                .with_help_message(
                    "The account where zero-dollar check matches or direct deposit splits will be posted",
                )
                .prompt()
                .context("Failed to select Net Zero Account")?;
        let net_zero_name = selected_net_zero
            .display_name
            .clone()
            .unwrap_or(selected_net_zero.name.clone());

        let default_payee = default_payslip_payee(kind);
        let payslip_payee = inquire::Text::new("Payee for created transactions:")
            .with_default(default_payee)
            .with_help_message("Stamped on the splits this importer creates for this provider")
            .prompt()
            .context("Failed to get payslip payee")?;

        // RSU plumbing only applies to providers that encode RSU vests as
        // separate $0 paychecks (Workday). Others fold stock comp inline, so we
        // neither prompt for nor emit those keys.
        let (rsu_account_line, rsu_payee_line) = if kind.uses_rsu_reconstruction() {
            let selected_rsu = inquire::Select::new("Select RSU Account:", accounts.clone())
                .with_help_message(
                    "The manual account used to track your RSU vests (e.g. Equity Awards)",
                )
                .prompt()
                .context("Failed to select RSU Account")?;
            let rsu_name = selected_rsu
                .display_name
                .clone()
                .unwrap_or(selected_rsu.name.clone());

            let rsu_payee_match = inquire::Text::new("RSU vest payee to match:")
                .with_default(default_rsu_payee_match(kind))
                .with_help_message(
                    "Payee of the auto-imported $0.00 RSU vest transaction (case-insensitive)",
                )
                .prompt()
                .context("Failed to get RSU payee match")?;

            (
                format!(
                    "# Manual account used to track RSU vests\nrsu_account = \"{}\"\n",
                    toml_escape(&rsu_name)
                ),
                format!(
                    "# Payee of the auto-imported $0.00 RSU vest transaction to match (case-insensitive)\nrsu_payee_match = \"{}\"\n",
                    toml_escape(&rsu_payee_match)
                ),
            )
        } else {
            (String::new(), String::new())
        };

        // This backend's mapping table.
        let mut mapping_toml = String::new();
        match per_backend_items.get(&kind) {
            Some(items) if !items.is_empty() => {
                for entry in items {
                    mapping_toml.push_str(&format!("\"{}\" = \"...\"\n", toml_escape(entry)));
                }
            }
            _ => {
                mapping_toml.push_str("# \"Salary\" = \"Salary\"\n");
                mapping_toml.push_str("# \"Federal Withholding\" = \"Taxes\"\n");
            }
        }
        let mapping_toml = mapping_toml.trim_end();

        backend_sections.push_str(&format!(
            r#"
[backends.{kind}]
# Account where zero-dollar check matches or direct deposit splits will be posted
net_zero_account = "{net_zero}"
# Payee for newly created direct deposit / net-zero transactions
payslip_payee = "{payee}"
{rsu_account_line}{rsu_payee_line}
[backends.{kind}.mapping]
# Map payslip item names to Lunch Money category names or IDs.
# Please manually modify this section to map each item to the correct category.
# TIP: Large Language Models (LLMs) are very good at filling in this categorization!
{mapping_toml}

[backends.{kind}.imputed_income]
# Line descriptions (exact, case-insensitive) that should NOT be treated as
# imputed income. Any description starting with '*' is always imputed income.
# exceptions = [
#     "relocation tax",
# ]
"#,
            kind = kind.as_str(),
            net_zero = toml_escape(&net_zero_name),
            payee = toml_escape(&payslip_payee),
        ));
    }

    let template = format!(
        r#"[lunch_money]
# Your Lunch Money developer API key
api_key = "{lunch_money_api_key}"
{tag_line}
{backend_sections}"#
    );

    fs::write(&output_path, template)
        .context(format!("Failed to write {}", output_path.display()))?;

    println! {};
    println! { "{STYLE_SUCCESS}🎉 Configuration created successfully!{STYLE_SUCCESS:#}" };
    println! { "{STYLE_INFO}Saved to:{STYLE_INFO:#} {}", output_path.display() };
    println! {};
    println! { "{STYLE_DIM}Run {STYLE_DIM:#}{STYLE_HEADER}lm-payslip-importer import <payslip_pdf>{STYLE_HEADER:#}{STYLE_DIM} to import your payslip.{STYLE_DIM:#}" };

    let total_seeded: usize = per_backend_items.values().map(|s| s.len()).sum();
    if total_seeded > 0 {
        println! {};
        let print_prompt = inquire::Confirm::new("Would you like to print a copy-pasteable LLM prompt to help you fill in these mappings?")
            .with_default(true)
            .with_help_message("This prompt lists your Lunch Money categories and the payslip items, making it easy for an LLM to categorize them.")
            .prompt()
            .context("Failed to get prompt printing preference")?;

        if print_prompt {
            let mut category_names: Vec<String> = lm_categories
                .iter()
                .filter(|c| !c.archived && !c.is_group)
                .map(|c| c.name.clone())
                .collect();
            category_names.sort();
            let categories_list = category_names.join("\n- ");

            // One prompt block per backend, each with that provider's mapping
            // header so the LLM output drops straight into the right section.
            let mut mapping_sections = String::new();
            for (kind, items) in &per_backend_items {
                if items.is_empty() {
                    continue;
                }
                mapping_sections.push_str(&format!("\n[backends.{}.mapping]\n", kind.as_str()));
                for entry in items {
                    mapping_sections
                        .push_str(&format!("\"{}\" = \"...\"\n", entry.replace('"', "\\\"")));
                }
            }

            let prompt_text = format!(
                r#"I need help mapping my payslip items to Lunch Money categories.

Here is the list of available Lunch Money categories:
- {}

Please map each of the following payslip items to the most appropriate Lunch Money category from the list above. The items are grouped by payroll provider. Return ONLY the completed TOML mapping entries, preserving each provider's section header exactly like this:
{}"#,
                categories_list, mapping_sections
            );

            println! {};
            println! { "{STYLE_HEADER}📋 Copy-Pasteable LLM Prompt:{STYLE_HEADER:#}" };
            println! { "{STYLE_DIM}─────────────────────────────────────────────────────────────────{STYLE_DIM:#}" };
            println! { "{prompt_text}" };
            println! { "{STYLE_DIM}─────────────────────────────────────────────────────────────────{STYLE_DIM:#}" };
            println! {};
        }
    }

    Ok(())
}

/// Default payee to suggest for a provider's created transactions.
fn default_payslip_payee(kind: crate::payslip::PayslipKind) -> &'static str {
    match kind {
        crate::payslip::PayslipKind::Workday => "Meta Payslip",
        crate::payslip::PayslipKind::Microsoft
        | crate::payslip::PayslipKind::AdpMicrosoft => "Microsoft Payslip",
    }
}

/// Default RSU vest payee to match for a provider (only meaningful for
/// providers that reconstruct RSU vests).
fn default_rsu_payee_match(kind: crate::payslip::PayslipKind) -> &'static str {
    match kind {
        crate::payslip::PayslipKind::Workday => "$META Vest",
        _ => "",
    }
}

/// Escape a string for embedding inside a double-quoted TOML basic string.
fn toml_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

