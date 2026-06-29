use anstream::eprintln;
use anstream::println;
use anyhow::Context;

use crate::style::*;

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

pub(crate) async fn run_init(
    args: crate::cli::InitArgs,
    output_path: std::path::PathBuf,
) -> anyhow::Result<()> {
    if args.just_categorize {
        if args.pdfs.is_empty() {
            anyhow::bail!(
                "No payslip PDF files were provided. Please specify one or more PDF files when using --just-categorize."
            );
        }

        // 1. Parse PDFs to gather unique items
        let per_backend_items = parse_pdfs(&args.pdfs)?;

        let doc = lm_common::config::editor::read_or_new(&output_path)?;

        // 2. Fetch categories by prompting for the API key if not in config
        let common_cfg = lm_common::config::common_section(&doc)?;
        let lunch_money_api_key = match common_cfg
            .lm_api_key
            .clone()
            .filter(|k| !k.trim().is_empty())
        {
            Some(key) => key,
            None => lm_common::init::prompt_lm_api_key()?,
        };
        let retry_policy = common_cfg.retry.clone();
        let lm_client = if !lunch_money_api_key.trim().is_empty() {
            println! { "{STYLE_INFO}🔗 Connecting to Lunch Money API to fetch categories...{STYLE_INFO:#}" };
            let http_client = reqwest::Client::new();
            Some(lunch_money::client::Client::new(
                http_client,
                lunch_money_api_key.trim().to_string(),
                retry_policy.into(),
            ))
        } else {
            None
        };

        // 3. Print LLM prompt
        print_llm_prompt(lm_client.as_ref(), &per_backend_items).await;

        return Ok(());
    }

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

    // Load the unified config if it already exists so we upsert the [payslip]
    // section (and the shared [common] key) in place, preserving every other
    // tool's section and all inline comments.
    let mut doc = lm_common::config::editor::read_or_new(&output_path)?;

    println! {};
    println! { "{STYLE_HEADER}⚙️  Configuring Lunch Money Payslip Importer{STYLE_HEADER:#}" };
    println! { "{STYLE_DIM}─────────────────────────────────────────────────────────────────{STYLE_DIM:#}" };
    println! { "{STYLE_INFO}This wizard will help you set up {}.{STYLE_INFO:#}", output_path.display() };
    println! {};

    let common_cfg = lm_common::config::common_section(&doc)?;
    let lunch_money_api_key = match common_cfg
        .lm_api_key
        .clone()
        .filter(|k| !k.trim().is_empty())
    {
        Some(key) => key,
        None => lm_common::init::prompt_lm_api_key()?,
    };

    println! {};
    println! { "{STYLE_INFO}🔗 Connecting to Lunch Money API...{STYLE_INFO:#}" };

    let http_client = reqwest::Client::new();
    let lm_client = lunch_money::client::Client::new(
        http_client.clone(),
        lunch_money_api_key.clone(),
        common_cfg.retry.into(),
    );

    let plaid_accts = lm_client
        .fetch_plaid_accounts()
        .await
        .context("Failed to fetch Plaid accounts")?;
    let manual_accts = lm_client
        .fetch_manual_accounts()
        .await
        .context("Failed to fetch manual accounts")?;

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
    let use_tag = inquire::Confirm::new(
        "Would you like to set an optional tag for transactions created by this importer?",
    )
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
    let per_backend_items = parse_pdfs(&args.pdfs)?;

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

        // Only providers that inject imputed-income offsets (Workday) detect
        // imputed lines by a marker and accept a list of extra unmarked
        // descriptions. Microsoft / ADP-Microsoft reconcile on their own, so
        // emitting the section for them would only produce a config the
        // validator rejects.
        let imputed_section = if kind.injects_imputed_offsets() {
            "\n[payslip.backends.{kind}.imputed_income]\n\
             # Extra line descriptions (exact, case-insensitive) to treat as imputed\n\
             # income, beyond those detected automatically. Any description starting\n\
             # with '*' is always imputed income; list here only the unmarked ones.\n\
             # descriptions = [\n\
             #     \"Relocation Tax Ben\",\n\
             # ]\n"
                .replace("{kind}", kind.as_str())
        } else {
            String::new()
        };

        backend_sections.push_str(&format!(
            r#"
[payslip.backends.{kind}]
# Account where zero-dollar check matches or direct deposit splits will be posted
net_zero_account = "{net_zero}"
# Payee for newly created direct deposit / net-zero transactions
payslip_payee = "{payee}"
{rsu_account_line}{rsu_payee_line}
[payslip.backends.{kind}.mapping]
# Map payslip item names to Lunch Money category names or IDs.
# Please manually modify this section to map each item to the correct category.
# TIP: Large Language Models (LLMs) are very good at filling in this categorization!
{mapping_toml}
{imputed_section}"#,
            kind = kind.as_str(),
            net_zero = toml_escape(&net_zero_name),
            payee = toml_escape(&payslip_payee),
        ));
    }

    let section_toml = format!(
        r#"[payslip]
{tag_line}
{backend_sections}"#
    );

    lm_common::config::editor::upsert_section(&mut doc, "payslip", &section_toml)?;
    lm_common::config::editor::ensure_common_section(&mut doc, lunch_money_api_key.trim());
    lm_common::config::editor::write_secure(&output_path, &doc)?;

    println! {};
    println! { "{STYLE_SUCCESS}🎉 Configuration created successfully!{STYLE_SUCCESS:#}" };
    println! { "{STYLE_INFO}Saved to:{STYLE_INFO:#} {}", output_path.display() };
    println! {};
    println! { "{STYLE_DIM}Run {STYLE_DIM:#}{STYLE_HEADER}lm-utils payslip-importer import <payslip_pdf>{STYLE_HEADER:#}{STYLE_DIM} to import your payslip.{STYLE_DIM:#}" };

    let total_seeded: usize = per_backend_items.values().map(|s| s.len()).sum();
    if total_seeded > 0 {
        println! {};
        let print_prompt = inquire::Confirm::new("Would you like to print a copy-pasteable LLM prompt to help you fill in these mappings?")
            .with_default(true)
            .with_help_message("This prompt lists your Lunch Money categories and the payslip items, making it easy for an LLM to categorize them.")
            .prompt()
            .context("Failed to get prompt printing preference")?;

        if print_prompt {
            print_llm_prompt(Some(&lm_client), &per_backend_items).await;
        }
    }

    Ok(())
}

/// Default payee to suggest for a provider's created transactions.
fn default_payslip_payee(kind: crate::payslip::PayslipKind) -> &'static str {
    match kind {
        crate::payslip::PayslipKind::Workday => "Meta Payslip",
        crate::payslip::PayslipKind::Microsoft | crate::payslip::PayslipKind::AdpMicrosoft => {
            "Microsoft Payslip"
        }
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

/// Parse the provided payslip PDFs to gather unique item descriptions.
fn parse_pdfs(
    pdfs: &[std::path::PathBuf],
) -> anyhow::Result<
    std::collections::BTreeMap<crate::payslip::PayslipKind, std::collections::BTreeSet<String>>,
> {
    use crate::payslip::PayslipKind;
    let mut per_backend_items: std::collections::BTreeMap<
        PayslipKind,
        std::collections::BTreeSet<String>,
    > = std::collections::BTreeMap::new();

    for pdf_path in pdfs {
        println! {};
        println! { "{STYLE_INFO}📄 Parsing payslip PDF to seed mappings: {}{STYLE_INFO:#}", pdf_path.display() };
        // Detect which payroll provider produced this PDF. If the provider
        // cannot be identified, error out.
        let kind = match crate::payslip::detect_kind(pdf_path) {
            Ok(Some(k)) => k,
            Ok(None) => {
                anyhow::bail!(
                    "Failed to identify payslip provider for '{}'.",
                    pdf_path.display()
                );
            }
            Err(e) => {
                anyhow::bail!(
                    "Failed to detect payslip provider for '{}': {}",
                    pdf_path.display(),
                    e
                );
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
    Ok(per_backend_items)
}

/// Print the copy-pasteable LLM prompt to stdout.
async fn print_llm_prompt(
    lm_client: Option<&lunch_money::client::Client>,
    per_backend_items: &std::collections::BTreeMap<
        crate::payslip::PayslipKind,
        std::collections::BTreeSet<String>,
    >,
) {
    let mut category_names = Vec::new();
    if let Some(lm_client) = lm_client {
        let cat_query = lunch_money::categories::query_params::CategoryQuery::builder()
            .format("flattened".to_string())
            .build();
        match lm_client.fetch_categories(&cat_query).await {
            Ok(lm_categories) => {
                category_names = lm_categories
                    .iter()
                    .filter(|c| !c.archived && !c.is_group)
                    .map(|c| {
                        let mut flags = Vec::new();
                        if c.is_income {
                            flags.push("treat as income");
                        }
                        if c.exclude_from_budget {
                            flags.push("exclude from budget");
                        }
                        if c.exclude_from_totals {
                            flags.push("exclude from totals");
                        }
                        if flags.is_empty() {
                            c.name.clone()
                        } else {
                            format!("{} ({})", c.name, flags.join(", "))
                        }
                    })
                    .collect();
                category_names.sort();
            }
            Err(e) => {
                eprintln! { "{STYLE_WARNING}⚠️  Warning: Failed to fetch categories from Lunch Money API: {}{STYLE_WARNING:#}", e };
            }
        }
    }

    let categories_list = if category_names.is_empty() {
        "[Insert your Lunch Money categories here]".to_string()
    } else {
        category_names.join("\n- ")
    };

    // One prompt block per backend, each with that provider's mapping
    // header so the LLM output drops straight into the right section.
    let mut mapping_sections = String::new();
    let mut imputed_instructions = String::new();
    for (kind, items) in per_backend_items {
        if items.is_empty() {
            continue;
        }
        mapping_sections.push_str(&format!("\n[payslip.backends.{}.mapping]\n", kind.as_str()));
        for entry in items {
            mapping_sections.push_str(&format!("\"{}\" = \"...\"\n", entry.replace('"', "\\\"")));
        }

        if kind.injects_imputed_offsets() {
            if imputed_instructions.is_empty() {
                imputed_instructions.push_str("\nAdditionally:\n");
            }
            imputed_instructions.push_str(&format!(
                "- If the `{}` payslip has unmarked imputed income items (taxable non-cash benefits that do NOT start with the backend's standard marker), please identify them and suggest adding their exact descriptions to the imputed income configuration block:\n\
                 ```toml\n\
                 [payslip.backends.{}.imputed_income]\n\
                 descriptions = [\n\
                     # list unmarked imputed income descriptions here\n\
                 ]\n\
                 ```\n",
                kind.as_str(),
                kind.as_str()
            ));
        }
    }

    let prompt_text = format!(
        r#"I need help mapping my payslip items to Lunch Money categories.

Here is the list of available Lunch Money categories (with flags indicating if they are treated as income, excluded from budget, or excluded from totals):
- {}

Please map each of the following payslip items to the most appropriate Lunch Money category from the list above. The items are grouped by payroll provider.

When choosing or recommending categories, keep these payroll rules in mind:
1. **Tax Withholdings**: Map taxes (federal/state/local withholdings, FICA/OASDI/Medicare, SDI, PFL) to dedicated expense categories (e.g. "Taxes" or similar).
2. **Imputed Income & Offsets**: Imputed income (taxable non-cash benefits like "*Imp GTL" or "Relocation Tax Ben") and their corresponding offset companion lines must share the exact same category, so their net cash flow impact is zero.
3. **Retirement & Transfers**: Pre-tax retirement deductions (e.g., 401k Salary) should be mapped to transfers/savings (e.g., "Payment, Transfer" or a dedicated "401k Transfer" category).
4. **RSU/Stock Vests**: Gross stock comp value (e.g., "Restricted Stock Units" or "STOCK AWARD INCOME") should map to gross income (e.g., "Salary" or a dedicated "Stock Vest" income category), while the matching Plaid transaction of $0.00 should map to a Transfer category (like a dedicated "Stock Vest" or "Stock Awards" transfer category) to keep the vest->sale story coherent and avoid double-counting.
5. **Deductions**: Pre-tax health deductions (e.g., Medical FSA, Pretax Dental, Pretax Medical) should map to appropriate benefit/insurance categories.{}

**CRITICAL INSTRUCTION**:
Prior to outputting the proposed TOML mapping, you MUST first:
1. List any suggested new categories that I should create (including their suggested names, group/type like Income/Expense/Transfer, specific settings like treat as income/exclude from budget/exclude from totals, and a brief justification based on the rules above).
2. Interactively ask me if I wish to stick to my existing categories (as best as possible) or if I want to use the new categories that you suggested.

Do NOT output the proposed TOML block until I reply to this question. Once I respond with my choice, you should then output the completed TOML mapping entries, preserving each provider's section header exactly like this (though, please organize the key-value pairs in the TOML mapping grouped by the Lunch Money category they are mapped to, rather than in alphabetical order by the payslip item names):
{}"#,
        categories_list, imputed_instructions, mapping_sections
    );

    println! {};
    println! { "{STYLE_HEADER}📋 Copy-Pasteable LLM Prompt:{STYLE_HEADER:#}" };
    println! { "{STYLE_DIM}─────────────────────────────────────────────────────────────────{STYLE_DIM:#}" };
    println! { "{prompt_text}" };
    println! { "{STYLE_DIM}─────────────────────────────────────────────────────────────────{STYLE_DIM:#}" };
    println! {};
}
