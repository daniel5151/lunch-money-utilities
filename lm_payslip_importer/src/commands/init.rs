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
    let selected_net_zero = inquire::Select::new("Select Net Zero Account:", accounts.clone())
        .with_help_message(
            "The account where zero-dollar check matches or direct deposit splits will be posted",
        )
        .prompt()
        .context("Failed to select Net Zero Account")?;
    let net_zero_name = selected_net_zero
        .display_name
        .clone()
        .unwrap_or(selected_net_zero.name.clone());

    println! {};
    let selected_rsu = inquire::Select::new("Select RSU Account:", accounts)
        .with_help_message("The manual account used to track your RSU vests (e.g. Equity Awards)")
        .prompt()
        .context("Failed to select RSU Account")?;
    let rsu_name = selected_rsu
        .display_name
        .clone()
        .unwrap_or(selected_rsu.name.clone());

    let mut mapping_entries = Vec::new();
    if !args.pdfs.is_empty() {
        let mut unique_items = std::collections::HashSet::new();
        for pdf_path in &args.pdfs {
            println! {};
            println! { "{STYLE_INFO}📄 Parsing payslip PDF to seed mappings: {}{STYLE_INFO:#}", pdf_path.display() };
            let pages = match crate::payslip::convert_pdf_to_pages(pdf_path) {
                Ok(p) => p,
                Err(e) => {
                    eprintln! { "{STYLE_ERROR}❌ Failed to extract pages from {}: {}{STYLE_ERROR:#}", pdf_path.display(), e };
                    continue;
                }
            };
            for (i, page_text) in pages.iter().enumerate() {
                let page_num = i + 1;
                let page_text = page_text.trim();
                if page_text.is_empty() {
                    continue;
                }
                if let Ok(parsed) = crate::payslip::parse_page_tables(page_text, page_num) {
                    // Only seed items that actually appeared with a non-zero
                    // *current* amount. YTD-only rows (zero current) would
                    // otherwise pollute the mapping list with items the user
                    // never needs to categorize (audit #10).
                    let mut collect = |rows: Vec<crate::payslip::RowData>| {
                        for item in rows {
                            let amount = item
                                .values
                                .get("Amount")
                                .copied()
                                .unwrap_or(rust_decimal::Decimal::ZERO);
                            if amount.is_zero() {
                                continue;
                            }
                            let desc = item.description.trim().to_string();
                            if !desc.is_empty() {
                                unique_items.insert(desc);
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
        let mut items: Vec<String> = unique_items.into_iter().collect();
        items.sort();
        mapping_entries = items;
    }

    let mut mapping_toml = String::new();
    if mapping_entries.is_empty() {
        mapping_toml.push_str("# \"Salary\" = \"Salary\"\n");
        mapping_toml.push_str("# \"Federal Withholding\" = \"Taxes\"\n");
    } else {
        for entry in &mapping_entries {
            let escaped = entry.replace('"', "\\\"");
            mapping_toml.push_str(&format!("\"{}\" = \"...\"\n", escaped));
        }
    }
    mapping_toml = mapping_toml.trim_end().to_string();

    let template = format!(
        r#"[lunch_money]
# Your Lunch Money developer API key
api_key = "{lunch_money_api_key}"
net_zero_account = "{net_zero_name}"
rsu_account = "{rsu_name}"

# Payee for newly created direct deposit / net-zero transactions
payslip_payee = "Meta Payslip"

# Payee name for auto-imported RSU vest events in Lunch Money (case-insensitive direct comparison)
rsu_payee_match = "$META Vest"

[mapping]
# Map payslip item names to Lunch Money category names or IDs.
# Please manually modify this section to map each item to the correct category.
# TIP: Large Language Models (LLMs) are very good at filling in this categorization!
{mapping_toml}

[imputed_income]
# List of imputed income payslip items that are exceptions and should not be treated as imputed income (e.g. relocation tax)
# exceptions = [
#     "relocation tax",
# ]
"#
    );

    fs::write(&output_path, template)
        .context(format!("Failed to write {}", output_path.display()))?;

    println! {};
    println! { "{STYLE_SUCCESS}🎉 Configuration created successfully!{STYLE_SUCCESS:#}" };
    println! { "{STYLE_INFO}Saved to:{STYLE_INFO:#} {}", output_path.display() };
    println! {};
    println! { "{STYLE_DIM}Run {STYLE_DIM:#}{STYLE_HEADER}lm-payslip-importer import <payslip_pdf>{STYLE_HEADER:#}{STYLE_DIM} to import your payslip.{STYLE_DIM:#}" };

    if !mapping_entries.is_empty() {
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
            let mut mapping_list = String::new();
            for entry in &mapping_entries {
                mapping_list.push_str(&format!("\"{}\" = \"...\"\n", entry.replace('"', "\\\"")));
            }

            let prompt_text = format!(
                r#"I need help mapping my payslip items to Lunch Money categories.

Here is the list of available Lunch Money categories:
- {}

Please map each of the following payslip items to the most appropriate Lunch Money category from the list above. Return ONLY the completed TOML mapping entries formatted exactly like this:

[mapping]
{}"#,
                categories_list, mapping_list
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
