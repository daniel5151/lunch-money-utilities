use anstream::eprintln;
use anstream::println;
use api::lunch_money::schema::DeletePayload;
use api::lunch_money::schema::InsertObject;
use api::lunch_money::schema::InsertPayload;
use api::lunch_money::schema::Transaction;
use api::lunch_money::schema::TransactionsResponse;
use api::lunch_money::schema::UpdateObject;
use api::lunch_money::schema::UpdatePayload;
use api::splitwise::schema::ExpensesResponse;
use api::splitwise::schema::GroupResponse;
use clap::Parser;
use clap::Subcommand;
use reqwest::Method;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::fs;

pub const STYLE_HEADER: anstyle::Style = anstyle::Style::new()
    .effects(anstyle::Effects::BOLD)
    .fg_color(Some(anstyle::Color::Ansi(anstyle::AnsiColor::BrightBlue)));

pub const STYLE_SUCCESS: anstyle::Style = anstyle::Style::new()
    .effects(anstyle::Effects::BOLD)
    .fg_color(Some(anstyle::Color::Ansi(anstyle::AnsiColor::Green)));

pub const STYLE_ERROR: anstyle::Style = anstyle::Style::new()
    .effects(anstyle::Effects::BOLD)
    .fg_color(Some(anstyle::Color::Ansi(anstyle::AnsiColor::Red)));

pub const STYLE_WARNING: anstyle::Style = anstyle::Style::new()
    .effects(anstyle::Effects::BOLD)
    .fg_color(Some(anstyle::Color::Ansi(anstyle::AnsiColor::Yellow)));

pub const STYLE_INFO: anstyle::Style =
    anstyle::Style::new().fg_color(Some(anstyle::Color::Ansi(anstyle::AnsiColor::Cyan)));

pub const STYLE_DIM: anstyle::Style =
    anstyle::Style::new().fg_color(Some(anstyle::Color::Ansi(anstyle::AnsiColor::BrightBlack)));

mod api;
mod config;

fn cli_styles() -> clap::builder::styling::Styles {
    clap::builder::styling::Styles::styled()
        .header(
            clap::builder::styling::Style::new()
                .bold()
                .fg_color(Some(clap::builder::styling::AnsiColor::BrightBlue.into())),
        )
        .usage(
            clap::builder::styling::Style::new()
                .bold()
                .fg_color(Some(clap::builder::styling::AnsiColor::BrightBlue.into())),
        )
        .literal(
            clap::builder::styling::Style::new()
                .fg_color(Some(clap::builder::styling::AnsiColor::Cyan.into())),
        )
        .placeholder(
            clap::builder::styling::Style::new()
                .fg_color(Some(clap::builder::styling::AnsiColor::BrightBlack.into())),
        )
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None, styles = cli_styles())]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Sync Splitwise transactions to Lunch Money
    Sync(SyncArgs),
    /// Initialize a new splitwise-lunchmoney.toml file with template data
    Init,
    /// Query data from Splitwise or Lunch Money
    Query(QueryArgs),
}

#[derive(Parser, Debug)]
pub struct QueryArgs {
    #[command(subcommand)]
    command: QuerySubcommands,
}

#[derive(Subcommand, Debug)]
pub enum QuerySubcommands {
    /// Query Splitwise expenses
    Splitwise(QuerySplitwiseArgs),
}

#[derive(Parser, Debug)]
pub struct QuerySplitwiseArgs {
    #[command(subcommand)]
    pub command: QuerySplitwiseSubcommands,
}

#[derive(Subcommand, Debug)]
pub enum QuerySplitwiseSubcommands {
    /// Query Splitwise expenses in a given time window
    Window(QuerySplitwiseWindowArgs),
    /// Query Splitwise expenses for a specific group
    Group(QuerySplitwiseGroupArgs),
    /// Get Splitwise groups as {id} - {friendlyname} pairs
    #[command(name = "get-groups")]
    GetGroups,
}

#[derive(Parser, Debug)]
pub struct QuerySplitwiseWindowArgs {
    /// Window duration for querying (e.g., "3 days", "24h", "1 week")
    #[arg(short, long, value_parser = humantime::parse_duration)]
    window: std::time::Duration,
}

#[derive(Parser, Debug)]
pub struct QuerySplitwiseGroupArgs {
    /// The Splitwise Group ID to filter by
    group_id: u64,
}

#[derive(Parser, Debug)]
pub struct SyncArgs {
    #[command(subcommand)]
    pub command: SyncSubcommands,
}

#[derive(Subcommand, Debug)]
pub enum SyncSubcommands {
    /// Sync transactions in a given time window
    Window(SyncWindowArgs),
    /// Sync all transactions corresponding to a specific Splitwise group
    Group(SyncGroupArgs),
}

#[derive(Parser, Debug)]
pub struct SyncWindowArgs {
    /// Window duration for synchronization (e.g., "3 days", "24h", "1 week")
    #[arg(short, long, value_parser = humantime::parse_duration)]
    pub window: std::time::Duration,

    /// Print what would be synced without modifying Lunch Money
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Parser, Debug)]
pub struct SyncGroupArgs {
    /// The Splitwise Group ID to synchronize
    pub group_id: u64,

    /// Print what would be synced without modifying Lunch Money
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(serde::Deserialize, Clone)]
struct SplitwiseUser {
    id: u64,
    first_name: String,
    last_name: Option<String>,
}

impl std::fmt::Display for SplitwiseUser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let last = self.last_name.as_deref().unwrap_or("");
        write!(f, "{} {} (ID: {})", self.first_name, last.trim(), self.id)
    }
}

#[derive(serde::Deserialize)]
struct CurrentUserResponse {
    user: SplitwiseUser,
}

#[derive(serde::Deserialize, Clone)]
struct ManualAccount {
    id: u64,
    name: String,
    display_name: Option<String>,
    #[serde(rename = "type")]
    account_type: String,
}

impl std::fmt::Display for ManualAccount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = self.display_name.as_deref().unwrap_or(&self.name);
        write!(f, "{} (ID: {})", name, self.id)
    }
}

#[derive(serde::Deserialize)]
struct ManualAccountsResponse {
    manual_accounts: Vec<ManualAccount>,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    match args.command {
        Commands::Init => {
            if std::path::Path::new("splitwise-lunchmoney.toml").exists() {
                eprintln!(
                    "\n{STYLE_ERROR}❌ Error:{STYLE_ERROR:#} splitwise-lunchmoney.toml already exists in this directory.\n"
                );
                std::process::exit(1);
            }

            println!(
                "\n{STYLE_HEADER}⚙️  Configuring Splitwise & Lunch Money Integration{STYLE_HEADER:#}"
            );
            println!(
                "{STYLE_DIM}─────────────────────────────────────────────────────────────────{STYLE_DIM:#}"
            );
            println!(
                "{STYLE_INFO}This wizard will help you set up splitwise-lunchmoney.toml.{STYLE_INFO:#}\n"
            );

            let splitwise_api_key = inquire::Password::new("Splitwise API Key:")
                .with_help_message("Your Splitwise personal API key / Bearer token")
                .with_display_mode(inquire::PasswordDisplayMode::Masked)
                .without_confirmation()
                .prompt()
                .expect("Failed to get Splitwise API Key");

            let http_client = reqwest::Client::new();

            println!("\n{STYLE_INFO}🔗 Connecting to Splitwise API...{STYLE_INFO:#}");
            let sw_user_response = http_client
                .get("https://secure.splitwise.com/api/v3.0/get_current_user")
                .header("Authorization", format!("Bearer {splitwise_api_key}"))
                .send()
                .await
                .expect("Failed to query Splitwise API. Please check your API key and internet connection.");

            if !sw_user_response.status().is_success() {
                eprintln!(
                    "\n{STYLE_ERROR}❌ Error querying Splitwise:{STYLE_ERROR:#} {}\n",
                    sw_user_response.status()
                );
                std::process::exit(1);
            }

            let user_res: CurrentUserResponse = sw_user_response
                .json()
                .await
                .expect("Failed to parse Splitwise current user response");

            let current_user = user_res.user;
            let selected_user = inquire::Select::new("Select Splitwise User:", vec![current_user])
                .prompt()
                .expect("Failed to select Splitwise User");

            let splitwise_user_id = selected_user.id;
            let splitwise_user_name = format!(
                "{} {}",
                selected_user.first_name,
                selected_user.last_name.as_deref().unwrap_or("")
            )
            .trim()
            .to_string();

            let lunch_money_api_key = inquire::Password::new("Lunch Money API Key:")
                .with_help_message("Your Lunch Money developer API key")
                .with_display_mode(inquire::PasswordDisplayMode::Masked)
                .without_confirmation()
                .prompt()
                .expect("Failed to get Lunch Money API Key");

            println!("\n{STYLE_INFO}🔗 Connecting to Lunch Money API...{STYLE_INFO:#}");
            let response = http_client
                .get("https://api.lunchmoney.dev/v2/manual_accounts")
                .header("Authorization", format!("Bearer {lunch_money_api_key}"))
                .send()
                .await
                .expect("Failed to query Lunch Money manual accounts. Please check your API key and internet connection.");

            if !response.status().is_success() {
                eprintln!(
                    "\n{STYLE_ERROR}❌ Error querying Lunch Money:{STYLE_ERROR:#} {}\n",
                    response.status()
                );
                std::process::exit(1);
            }

            let accounts_res: ManualAccountsResponse = response
                .json()
                .await
                .expect("Failed to parse Lunch Money manual accounts response");

            let mut supported_currencies = Vec::new();
            println!("\n{STYLE_INFO}💱 Supported Currencies Setup{STYLE_INFO:#}");
            println!(
                "{STYLE_DIM}Please enter the currencies you want to support (e.g. USD, CAD, GBP).{STYLE_DIM:#}"
            );
            loop {
                let prompt_msg = if supported_currencies.is_empty() {
                    "Enter a currency code you would like to support:"
                } else {
                    "Enter another currency code, or press Enter/leave blank to finish:"
                };
                let currency = inquire::Text::new(prompt_msg)
                    .prompt()
                    .expect("Failed to get currency code");
                let trimmed = currency.trim().to_uppercase();
                if trimmed.is_empty() {
                    if supported_currencies.is_empty() {
                        println!("At least one currency must be specified.");
                        continue;
                    }
                    break;
                }
                if trimmed.len() != 3 || !trimmed.chars().all(|c| c.is_ascii_alphabetic()) {
                    println!("Please enter a valid 3-letter ISO currency code (e.g. USD).");
                    continue;
                }
                if !supported_currencies.contains(&trimmed) {
                    supported_currencies.push(trimmed);
                }
            }

            let mut target_accounts = HashMap::new();
            let mut missing_accounts = Vec::new();

            for currency in &supported_currencies {
                let expected_name = format!("Splitwise {}", currency);
                if let Some(acc) = accounts_res
                    .manual_accounts
                    .iter()
                    .find(|acc| acc.name.eq_ignore_ascii_case(&expected_name))
                {
                    target_accounts.insert(currency.clone(), acc.id);
                } else {
                    missing_accounts.push(expected_name);
                }
            }

            if !missing_accounts.is_empty() {
                eprintln!(
                    "\n{STYLE_ERROR}❌ Error:{STYLE_ERROR:#} The following required Lunch Money manual accounts are missing:"
                );
                for acc_name in &missing_accounts {
                    eprintln!("  • {STYLE_HEADER}{}{STYLE_HEADER:#}", acc_name);
                }
                eprintln!(
                    "\n{STYLE_WARNING}⚠️  Action Required:{STYLE_WARNING:#} Please set up manually managed accounts with these exact names in your Lunch Money account before continuing.\n"
                );
                std::process::exit(1);
            }

            let mut target_accounts_toml = String::new();
            for (curr, id) in &target_accounts {
                target_accounts_toml.push_str(&format!("{} = {}\n", curr, id));
            }

            let template = format!(
                r#"[splitwise]
# Your personal Splitwise API key
api_key = "{splitwise_api_key}"

# Your Splitwise user ID
user_id = {splitwise_user_id} # {splitwise_user_name}

# Array of Splitwise group IDs to ignore (optional)
# ignored_groups = [123456, 789012]

[lunch_money]
# Your Lunch Money developer API key
api_key = "{lunch_money_api_key}"

# The mapping from currency code to manual account ID in Lunch Money
[lunch_money.target_accounts]
{target_accounts_toml}"#
            );

            fs::write("splitwise-lunchmoney.toml", template)
                .expect("Failed to write splitwise-lunchmoney.toml");

            println!(
                "\n{STYLE_SUCCESS}🎉 Configuration created successfully!{STYLE_SUCCESS:#}\n\
                 {STYLE_INFO}Saved to:{STYLE_INFO:#} splitwise-lunchmoney.toml\n\n\
                 {STYLE_DIM}Run {STYLE_DIM:#}{STYLE_HEADER}splitwise-lunchmoney sync window --window \"3 days\"{STYLE_HEADER:#}{STYLE_DIM} to begin syncing.{STYLE_DIM:#}\n"
            );
        }
        Commands::Sync(sync_args) => match sync_args.command {
            SyncSubcommands::Window(args) => {
                run_sync_window(args).await;
            }
            SyncSubcommands::Group(args) => {
                run_sync_group(args).await;
            }
        },
        Commands::Query(query_args) => match query_args.command {
            QuerySubcommands::Splitwise(splitwise_args) => match splitwise_args.command {
                QuerySplitwiseSubcommands::Window(args) => {
                    run_query_splitwise_window(args).await;
                }
                QuerySplitwiseSubcommands::Group(args) => {
                    run_query_splitwise_group(args).await;
                }
                QuerySplitwiseSubcommands::GetGroups => {
                    run_query_splitwise_get_groups().await;
                }
            },
        },
    }
}

async fn run_query_splitwise_window(args: QuerySplitwiseWindowArgs) {
    let window_duration =
        jiff::SignedDuration::try_from(args.window).expect("window duration is too large");

    let config_content = fs::read_to_string("splitwise-lunchmoney.toml")
        .expect("Failed to read splitwise-lunchmoney.toml from current working directory");
    let config: config::Config =
        toml::from_str(&config_content).expect("Malformed splitwise-lunchmoney.toml file");

    let http_pool = reqwest::Client::new();
    let sw_client =
        api::splitwise::Client::new(http_pool.clone(), config.splitwise.api_key.clone());

    let start_window = jiff::Timestamp::now() - window_duration;
    let start_window_str = start_window
        .to_zoned(jiff::tz::TimeZone::UTC)
        .strftime("%Y-%m-%d")
        .to_string();

    let bar = "─".repeat(92);

    println!("\n{STYLE_HEADER}🔍 Querying Splitwise Expenses{STYLE_HEADER:#}");
    println!("{STYLE_DIM}{bar}{STYLE_DIM:#}");
    println!(
        "{STYLE_INFO}📅 Window boundary:{STYLE_INFO:#} {}",
        start_window_str
    );
    println!();

    println!("  {STYLE_DIM}Fetching Splitwise groups and expenses...{STYLE_DIM:#}");
    let groups_res: GroupResponse = sw_client.fetch("get_groups", &[] as &[(&str, &str)]).await;
    let group_map: HashMap<u64, String> = groups_res
        .groups
        .into_iter()
        .map(|g| (g.id, g.name))
        .collect();

    let sw_query = [("dated_after", start_window_str.as_str()), ("limit", "0")];
    let expenses_res: ExpensesResponse = sw_client.fetch("get_expenses", &sw_query).await;

    if expenses_res.expenses.is_empty() {
        println!("{STYLE_SUCCESS}✨ No expenses found in this window.{STYLE_SUCCESS:#}\n");
        return;
    }

    println!(
        "  {:<10}  {:<30}  {:<30}  {:>12}",
        "Date", "Group/Person", "Description", "Net Balance"
    );
    println!("  {STYLE_DIM}{bar}{STYLE_DIM:#}");

    let mut has_uninvolved = false;

    for expense in expenses_res.expenses {
        let net_balance = expense
            .users
            .iter()
            .find(|u| u.user_id == config.splitwise.user_id)
            .map(|u| u.net_balance)
            .unwrap_or(Decimal::ZERO);

        let date_str = expense
            .date
            .to_zoned(jiff::tz::TimeZone::UTC)
            .date()
            .strftime("%Y-%m-%d")
            .to_string();

        let payee_str = match expense.group_id {
            Some(gid) => group_map
                .get(&gid)
                .cloned()
                .unwrap_or_else(|| "Unknown Group".to_string()),
            None => expense
                .users
                .iter()
                .find(|u| u.user_id != config.splitwise.user_id)
                .and_then(|u| u.user.as_ref())
                .map(|d| {
                    format!(
                        "{} {}",
                        d.first_name.as_deref().unwrap_or(""),
                        d.last_name.as_deref().unwrap_or("")
                    )
                    .trim()
                    .to_string()
                })
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "Non-group".to_string()),
        };

        let mut clean_payee = payee_str;
        if clean_payee.len() > 30 {
            clean_payee.truncate(27);
            clean_payee.push_str("...");
        }

        let is_ignored = expense
            .group_id
            .is_some_and(|gid| config.splitwise.ignored_groups.contains(&gid));

        // Styling and status tag
        let (style, status_tag, is_uninvolved) = if expense.deleted_at.is_some() {
            (STYLE_DIM, " [DELETED]", false)
        } else if is_ignored {
            (STYLE_WARNING, " [IGNORED]", false)
        } else if net_balance.is_zero() {
            has_uninvolved = true;
            (STYLE_DIM, "", true)
        } else if net_balance.is_sign_negative() {
            (STYLE_ERROR, "", false)
        } else {
            (STYLE_SUCCESS, "", false)
        };

        // Determine max allowed length for description, so description + status_tag is exactly 30 visible chars
        let max_desc_len = 30_usize.saturating_sub(status_tag.len());
        let mut clean_desc = expense.description.trim().to_string();
        if clean_desc.len() > max_desc_len {
            let truncate_to = max_desc_len.saturating_sub(3);
            clean_desc.truncate(truncate_to);
            clean_desc = format!("{}...", clean_desc.trim_end());
        }

        let balance_plain = format!("{:>12}", net_balance);
        let balance_colored = format!("{}{}{:#}", style, balance_plain, style);

        let desc_colored = if !status_tag.is_empty() {
            let padding_spaces =
                " ".repeat(30_usize.saturating_sub(clean_desc.len() + status_tag.len()));
            format!(
                "{}{STYLE_DIM}{status_tag}{STYLE_DIM:#}{}",
                clean_desc, padding_spaces
            )
        } else {
            format!("{:<30}", clean_desc)
        };

        let currency_suffix = if is_uninvolved {
            format!("{}*", expense.currency_code.to_uppercase())
        } else {
            expense.currency_code.to_uppercase()
        };

        println!(
            "  {:<10}  {:<30}  {}  {} {}",
            date_str, clean_payee, desc_colored, balance_colored, currency_suffix
        );
    }

    if has_uninvolved {
        println!("  {STYLE_DIM}* = uninvolved transaction (net balance is zero){STYLE_DIM:#}\n");
    } else {
        println!();
    }
}

async fn run_query_splitwise_group(args: QuerySplitwiseGroupArgs) {
    let config_content = fs::read_to_string("splitwise-lunchmoney.toml")
        .expect("Failed to read splitwise-lunchmoney.toml from current working directory");
    let config: config::Config =
        toml::from_str(&config_content).expect("Malformed splitwise-lunchmoney.toml file");

    let http_pool = reqwest::Client::new();
    let sw_client =
        api::splitwise::Client::new(http_pool.clone(), config.splitwise.api_key.clone());

    let bar = "─".repeat(92);

    println!("\n{STYLE_HEADER}🔍 Querying Splitwise Group Expenses{STYLE_HEADER:#}");
    println!("{STYLE_DIM}{bar}{STYLE_DIM:#}");

    println!("  {STYLE_DIM}Fetching Splitwise groups and expenses...{STYLE_DIM:#}");
    let groups_res: GroupResponse = sw_client.fetch("get_groups", &[] as &[(&str, &str)]).await;
    let group_map: HashMap<u64, String> = groups_res
        .groups
        .iter()
        .map(|g| (g.id, g.name.clone()))
        .collect();

    let target_group = groups_res.groups.iter().find(|g| g.id == args.group_id);

    let group_name = target_group
        .map(|g| g.name.clone())
        .unwrap_or_else(|| "Unknown Group".to_string());

    println!(
        "{STYLE_INFO}👥 Group:{STYLE_INFO:#} {} (ID: {})",
        group_name, args.group_id
    );
    if let Some(g) = target_group {
        let balance_str = format_group_balances(g, config.splitwise.user_id);
        println!("{STYLE_INFO}💰 Balance:{STYLE_INFO:#} {}", balance_str);
    }
    println!();

    let group_id_str = args.group_id.to_string();
    let sw_query = [("group_id", group_id_str.as_str()), ("limit", "0")];
    let expenses_res: ExpensesResponse = sw_client.fetch("get_expenses", &sw_query).await;

    if expenses_res.expenses.is_empty() {
        println!("{STYLE_SUCCESS}✨ No expenses found for this group.{STYLE_SUCCESS:#}\n");
        return;
    }

    println!(
        "  {:<10}  {:<30}  {:<30}  {:>12}",
        "Date", "Group/Person", "Description", "Net Balance"
    );
    println!("  {STYLE_DIM}{bar}{STYLE_DIM:#}");

    let mut has_uninvolved = false;

    for expense in expenses_res.expenses {
        let net_balance = expense
            .users
            .iter()
            .find(|u| u.user_id == config.splitwise.user_id)
            .map(|u| u.net_balance)
            .unwrap_or(Decimal::ZERO);

        let date_str = expense
            .date
            .to_zoned(jiff::tz::TimeZone::UTC)
            .date()
            .strftime("%Y-%m-%d")
            .to_string();

        let payee_str = match expense.group_id {
            Some(gid) => group_map
                .get(&gid)
                .cloned()
                .unwrap_or_else(|| "Unknown Group".to_string()),
            None => expense
                .users
                .iter()
                .find(|u| u.user_id != config.splitwise.user_id)
                .and_then(|u| u.user.as_ref())
                .map(|d| {
                    format!(
                        "{} {}",
                        d.first_name.as_deref().unwrap_or(""),
                        d.last_name.as_deref().unwrap_or("")
                    )
                    .trim()
                    .to_string()
                })
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "Non-group".to_string()),
        };

        let mut clean_payee = payee_str;
        if clean_payee.len() > 30 {
            clean_payee.truncate(27);
            clean_payee.push_str("...");
        }

        let is_ignored = expense
            .group_id
            .is_some_and(|gid| config.splitwise.ignored_groups.contains(&gid));

        // Styling and status tag
        let (style, status_tag, is_uninvolved) = if expense.deleted_at.is_some() {
            (STYLE_DIM, " [DELETED]", false)
        } else if is_ignored {
            (STYLE_WARNING, " [IGNORED]", false)
        } else if net_balance.is_zero() {
            has_uninvolved = true;
            (STYLE_DIM, "", true)
        } else if net_balance.is_sign_negative() {
            (STYLE_ERROR, "", false)
        } else {
            (STYLE_SUCCESS, "", false)
        };

        // Determine max allowed length for description, so description + status_tag is exactly 30 visible chars
        let max_desc_len = 30_usize.saturating_sub(status_tag.len());
        let mut clean_desc = expense.description.trim().to_string();
        if clean_desc.len() > max_desc_len {
            let truncate_to = max_desc_len.saturating_sub(3);
            clean_desc.truncate(truncate_to);
            clean_desc = format!("{}...", clean_desc.trim_end());
        }

        let balance_plain = format!("{:>12}", net_balance);
        let balance_colored = format!("{}{}{:#}", style, balance_plain, style);

        let desc_colored = if !status_tag.is_empty() {
            let padding_spaces =
                " ".repeat(30_usize.saturating_sub(clean_desc.len() + status_tag.len()));
            format!(
                "{}{STYLE_DIM}{status_tag}{STYLE_DIM:#}{}",
                clean_desc, padding_spaces
            )
        } else {
            format!("{:<30}", clean_desc)
        };

        let currency_suffix = if is_uninvolved {
            format!("{}*", expense.currency_code.to_uppercase())
        } else {
            expense.currency_code.to_uppercase()
        };

        println!(
            "  {:<10}  {:<30}  {}  {} {}",
            date_str, clean_payee, desc_colored, balance_colored, currency_suffix
        );
    }

    if has_uninvolved {
        println!("  {STYLE_DIM}* = uninvolved transaction (net balance is zero){STYLE_DIM:#}\n");
    } else {
        println!();
    }
}

async fn run_query_splitwise_get_groups() {
    let config_content = fs::read_to_string("splitwise-lunchmoney.toml")
        .expect("Failed to read splitwise-lunchmoney.toml from current working directory");
    let config: config::Config =
        toml::from_str(&config_content).expect("Malformed splitwise-lunchmoney.toml file");

    let http_pool = reqwest::Client::new();
    let sw_client =
        api::splitwise::Client::new(http_pool.clone(), config.splitwise.api_key.clone());

    let bar = "─".repeat(110);

    println!("\n{STYLE_HEADER}🔍 Querying Splitwise Groups{STYLE_HEADER:#}");
    println!("{STYLE_DIM}{bar}{STYLE_DIM:#}");

    let groups_res: GroupResponse = sw_client.fetch("get_groups", &[] as &[(&str, &str)]).await;

    if groups_res.groups.is_empty() {
        println!("{STYLE_WARNING}No groups found.{STYLE_WARNING:#}\n");
        return;
    }

    println!(
        "  {:<15}  {:<15}  {:<40}  {}",
        "Last Updated", "Group ID", "Group Name", "Balance"
    );
    println!("  {STYLE_DIM}{bar}{STYLE_DIM:#}");

    let mut groups = groups_res.groups;
    groups.sort_by_key(|b| std::cmp::Reverse(b.updated_at));

    for g in groups {
        let mut clean_name = g.name.clone();
        if clean_name.len() > 40 {
            clean_name.truncate(37);
            clean_name.push_str("...");
        }
        let date_str = g
            .updated_at
            .to_zoned(jiff::tz::TimeZone::UTC)
            .date()
            .strftime("%Y-%m-%d")
            .to_string();
        let balance_str = format_group_balances(&g, config.splitwise.user_id);
        println!(
            "  {:<15}  {:<15}  {:<40}  {}",
            date_str, g.id, clean_name, balance_str
        );
    }
    println!();
}

fn format_transaction_summary(
    payee: &str,
    amount: Decimal,
    currency: &str,
    date: jiff::civil::Date,
    notes: &str,
    account_name: &str,
) -> String {
    let date_str = date.strftime("%Y-%m-%d").to_string();
    let currency_upper = currency.to_uppercase();
    let amount_style = if amount.is_sign_negative() {
        STYLE_ERROR
    } else {
        STYLE_SUCCESS
    };

    // Limit payee length to 35 characters for clean alignment, appending '...' if truncated
    let mut clean_payee = payee.to_string();
    if clean_payee.len() > 35 {
        clean_payee.truncate(32);
        clean_payee.push_str("...");
    }

    let trimmed_notes = notes.trim();
    let notes_suffix = if trimmed_notes.is_empty() {
        "".to_string()
    } else {
        format!("  {}{}{:#}", STYLE_DIM, trimmed_notes, STYLE_DIM)
    };

    let account_display = if account_name.is_empty() {
        "".to_string()
    } else {
        format!("  {}[{}]{:#}", STYLE_INFO, account_name, STYLE_INFO)
    };

    format!(
        "{}  {:<35}  {}{:>9} {}{:#}{}{}",
        date_str, clean_payee, amount_style, amount, currency_upper, amount_style, account_display, notes_suffix
    )
}

async fn run_sync_window(sync_args: SyncWindowArgs) {
    let window_duration =
        jiff::SignedDuration::try_from(sync_args.window).expect("window duration is too large");

    let config_content = fs::read_to_string("splitwise-lunchmoney.toml")
        .expect("Failed to read splitwise-lunchmoney.toml from current working directory");
    let config: config::Config =
        toml::from_str(&config_content).expect("Malformed splitwise-lunchmoney.toml file");

    let http_pool = reqwest::Client::new();
    let sw_client =
        api::splitwise::Client::new(http_pool.clone(), config.splitwise.api_key.clone());
    let lm_client = api::lunch_money::Client::new(http_pool, config.lunch_money.api_key.clone());

    let start_window = jiff::Timestamp::now() - window_duration;
    let start_window_str = start_window
        .to_zoned(jiff::tz::TimeZone::UTC)
        .strftime("%Y-%m-%d")
        .to_string();

    let end_window_str = jiff::Timestamp::now()
        .to_zoned(jiff::tz::TimeZone::UTC)
        .strftime("%Y-%m-%d")
        .to_string();

    let dry_run_suffix = if sync_args.dry_run {
        format!(" {STYLE_WARNING}[DRY RUN]{STYLE_WARNING:#}")
    } else {
        "".to_string()
    };
    println!(
        "\n{STYLE_HEADER}⚡ Splitwise to Lunch Money Sync{}{STYLE_HEADER:#}",
        dry_run_suffix
    );
    println!("{STYLE_DIM}──────────────────────────────────────────────────{STYLE_DIM:#}");
    println!(
        "{STYLE_INFO}📅 Sync window boundary:{STYLE_INFO:#} {} to {}",
        start_window_str, end_window_str
    );
    println!();

    // Fetch dependencies
    println!("  {STYLE_DIM}Fetching Splitwise groups and expenses...{STYLE_DIM:#}");
    let groups_res: GroupResponse = sw_client.fetch("get_groups", &[] as &[(&str, &str)]).await;
    let group_map: HashMap<u64, String> = groups_res
        .groups
        .into_iter()
        .map(|g| (g.id, g.name))
        .collect();

    let sw_query = [("dated_after", start_window_str.as_str()), ("limit", "0")];
    let expenses_res: ExpensesResponse = sw_client.fetch("get_expenses", &sw_query).await;

    // Verify configured manual accounts exist in Lunch Money
    let accounts_res: ManualAccountsResponse = lm_client
        .fetch("manual_accounts", &[] as &[(&str, &str)])
        .await;
    for (currency, &account_id) in &config.lunch_money.target_accounts {
        if !accounts_res.manual_accounts.iter().any(|acc| acc.id == account_id) {
            eprintln!(
                "\n{STYLE_ERROR}❌ Error:{STYLE_ERROR:#} Configured manual account ID {} for currency '{}' has been deleted or does not exist in Lunch Money.\n\
                 Please check your Lunch Money manual accounts or run 'splitwise-lunchmoney init'.\n",
                account_id, currency
            );
            std::process::exit(1);
        }
    }

    let get_account_name = |manual_account_id: Option<u64>, currency: &str| -> String {
        let id = manual_account_id.or_else(|| {
            let currency_upper = currency.to_uppercase();
            config.lunch_money.target_accounts.get(&currency_upper).copied()
        });
        if let Some(id) = id {
            if let Some(acc) = accounts_res.manual_accounts.iter().find(|acc| acc.id == id) {
                return acc.display_name.as_deref().unwrap_or(&acc.name).to_string();
            }
        }
        "Unknown Account".to_string()
    };

    println!("  {STYLE_DIM}Fetching Lunch Money transactions...{STYLE_DIM:#}");
    let mut lm_transactions = Vec::new();
    for &account_id in config.lunch_money.target_accounts.values() {
        let account_id_str = account_id.to_string();
        let lm_query = [
            ("start_date", start_window_str.as_str()),
            ("end_date", end_window_str.as_str()),
            ("manual_account_id", account_id_str.as_str()),
            ("limit", "1000"),
        ];
        let lm_res: TransactionsResponse = lm_client.fetch("transactions", &lm_query).await;
        let is_loan = accounts_res
            .manual_accounts
            .iter()
            .find(|acc| acc.id == account_id)
            .map(|acc| acc.account_type == "loan")
            .unwrap_or(false);

        let mut txs = lm_res.transactions;
        if is_loan {
            for t in &mut txs {
                t.amount = -t.amount;
            }
        }
        lm_transactions.extend(txs);
    }

    println!("  {STYLE_DIM}Comparing transactions...{STYLE_DIM:#}\n");

    let mut lm_map: HashMap<String, Transaction> = lm_transactions
        .into_iter()
        .filter_map(|t| t.external_id.clone().map(|ext_id| (ext_id, t)))
        .collect();

    // Prepare batch operations
    let mut inserts: Vec<InsertObject> = Vec::new();
    let mut updates: Vec<UpdateObject> = Vec::new();
    let mut deletes: Vec<Transaction> = Vec::new();

    for expense in expenses_res.expenses {
        let external_id = format!("splitwise_{}", expense.id);

        let net_balance = expense
            .users
            .iter()
            .find(|u| u.user_id == config.splitwise.user_id)
            .map(|u| u.net_balance) // Automatically typed as Decimal by serde!
            .unwrap_or(Decimal::ZERO);

        let is_ignored = expense
            .group_id
            .is_some_and(|gid| config.splitwise.ignored_groups.contains(&gid));

        // Skip ignored, deleted, or un-involved expenses
        if expense.deleted_at.is_some() || is_ignored || net_balance.is_zero() {
            if let Some(existing_lm) = lm_map.remove(&external_id) {
                deletes.push(existing_lm);
            }
            continue;
        }

        let currency_upper = expense.currency_code.to_uppercase();
        if !config
            .lunch_money
            .target_accounts
            .contains_key(&currency_upper)
        {
            eprintln!(
                "\n{STYLE_ERROR}❌ Error:{STYLE_ERROR:#} No manual account configured for currency '{}'.\n\
                 Please run 'splitwise-lunchmoney init' or set up 'Splitwise {}' manual account.\n",
                currency_upper, currency_upper
            );
            std::process::exit(1);
        }

        let date_civil = expense.date.to_zoned(jiff::tz::TimeZone::UTC).date();
        let currency_lower = expense.currency_code.to_lowercase();

        let payee_str = format!(
            "Splitwise - {}",
            match expense.group_id {
                Some(gid) => group_map
                    .get(&gid)
                    .cloned()
                    .unwrap_or_else(|| "Unknown Group".to_string()),
                None => expense
                    .users
                    .iter()
                    .find(|u| u.user_id != config.splitwise.user_id)
                    .and_then(|u| u.user.as_ref())
                    .map(|d| {
                        format!(
                            "{} {}",
                            d.first_name.as_deref().unwrap_or(""),
                            d.last_name.as_deref().unwrap_or("")
                        )
                        .trim()
                        .to_string()
                    })
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| "Non-group".to_string()),
            }
        );

        if let Some(existing_lm) = lm_map.remove(&external_id) {
            // Strict exact-match diffing without float approximations
            let amount_changed = existing_lm.amount != net_balance;
            let notes_changed = existing_lm.notes.as_deref().unwrap_or("") != expense.description;

            if amount_changed
                || existing_lm.date != date_civil
                || existing_lm.currency != currency_lower
                || existing_lm.payee != payee_str
                || notes_changed
            {
                updates.push(UpdateObject {
                    id: existing_lm.id,
                    date: date_civil,
                    amount: net_balance,
                    currency: currency_lower,
                    payee: payee_str,
                    notes: expense.description,
                });
            }
        } else {
            let manual_account_id = config.lunch_money.target_accounts[&currency_upper];
            inserts.push(InsertObject {
                date: date_civil,
                amount: net_balance,
                currency: currency_lower,
                payee: payee_str,
                notes: expense.description,
                external_id,
                manual_account_id,
                status: "reviewed".to_string(),
            });
        }
    }

    // Execute batches
    if !deletes.is_empty() {
        println!(
            "🗑️  {STYLE_WARNING}Deleting {STYLE_WARNING:#}{} old/modified transaction(s) from Lunch Money:",
            deletes.len()
        );
        for t in &deletes {
            let acc_name = get_account_name(t.manual_account_id, &t.currency);
            println!(
                "   {STYLE_ERROR}-{STYLE_ERROR:#} {}",
                format_transaction_summary(&t.payee, t.amount, &t.currency, t.date, t.notes.as_deref().unwrap_or(""), &acc_name)
            );
        }
        println!();

        if !sync_args.dry_run {
            let delete_ids: Vec<u64> = deletes.iter().map(|t| t.id).collect();
            lm_client
                .exec(
                    Method::DELETE,
                    "transactions",
                    &DeletePayload { ids: delete_ids },
                )
                .await;
        }
    }

    if !updates.is_empty() {
        println!(
            "✎  {STYLE_INFO}Updating {STYLE_INFO:#}{} modified transaction(s) in Lunch Money:",
            updates.len()
        );
        for u in &updates {
            let acc_name = get_account_name(None, &u.currency);
            println!(
                "   {STYLE_INFO}~{STYLE_INFO:#} {}",
                format_transaction_summary(&u.payee, u.amount, &u.currency, u.date, &u.notes, &acc_name)
            );
        }
        println!();

        if !sync_args.dry_run {
            for chunk in updates.chunks(500) {
                let mut chunk_txs = chunk.to_vec();
                for u in &mut chunk_txs {
                    let is_loan = accounts_res
                        .manual_accounts
                        .iter()
                        .find(|acc| {
                            let curr = u.currency.to_uppercase();
                            config.lunch_money.target_accounts.get(&curr).copied() == Some(acc.id)
                        })
                        .map(|acc| acc.account_type == "loan")
                        .unwrap_or(false);
                    if is_loan {
                        u.amount = -u.amount;
                    }
                }
                lm_client
                    .exec(
                        Method::PUT,
                        "transactions",
                        &UpdatePayload {
                            transactions: chunk_txs,
                        },
                    )
                    .await;
            }
        }
    }

    if !inserts.is_empty() {
        println!(
            "✓  {STYLE_SUCCESS}Inserting {STYLE_SUCCESS:#}{} new transaction(s) to Lunch Money:",
            inserts.len()
        );
        for ins in &inserts {
            let acc_name = get_account_name(Some(ins.manual_account_id), &ins.currency);
            println!(
                "   {STYLE_SUCCESS}+{STYLE_SUCCESS:#} {}",
                format_transaction_summary(&ins.payee, ins.amount, &ins.currency, ins.date, &ins.notes, &acc_name)
            );
        }
        println!();

        if !sync_args.dry_run {
            for chunk in inserts.chunks(500) {
                let mut chunk_txs = chunk.to_vec();
                for ins in &mut chunk_txs {
                    let is_loan = accounts_res
                        .manual_accounts
                        .iter()
                        .find(|acc| acc.id == ins.manual_account_id)
                        .map(|acc| acc.account_type == "loan")
                        .unwrap_or(false);
                    if is_loan {
                        ins.amount = -ins.amount;
                    }
                }
                lm_client
                    .exec(
                        Method::POST,
                        "transactions",
                        &InsertPayload {
                            transactions: chunk_txs,
                        },
                    )
                    .await;
            }
        }
    }

    if deletes.is_empty() && updates.is_empty() && inserts.is_empty() {
        println!(
            "{STYLE_SUCCESS}✨ No changes detected. Lunch Money manual account is up-to-date!{STYLE_SUCCESS:#}\n"
        );
    } else if sync_args.dry_run {
        println!(
            "{STYLE_WARNING}⚠️ Dry run complete! No changes were made to Lunch Money.{STYLE_WARNING:#}\n"
        );
    } else {
        println!("{STYLE_SUCCESS}✨ Synchronization cycle complete!{STYLE_SUCCESS:#}\n");
    }
}

async fn run_sync_group(sync_args: SyncGroupArgs) {
    let config_content = fs::read_to_string("splitwise-lunchmoney.toml")
        .expect("Failed to read splitwise-lunchmoney.toml from current working directory");
    let config: config::Config =
        toml::from_str(&config_content).expect("Malformed splitwise-lunchmoney.toml file");

    let http_pool = reqwest::Client::new();
    let sw_client =
        api::splitwise::Client::new(http_pool.clone(), config.splitwise.api_key.clone());
    let lm_client = api::lunch_money::Client::new(http_pool, config.lunch_money.api_key.clone());

    let dry_run_suffix = if sync_args.dry_run {
        format!(" {STYLE_WARNING}[DRY RUN]{STYLE_WARNING:#}")
    } else {
        "".to_string()
    };
    println!(
        "\n{STYLE_HEADER}⚡ Splitwise to Lunch Money Sync Group{}{STYLE_HEADER:#}",
        dry_run_suffix
    );
    println!("{STYLE_DIM}──────────────────────────────────────────────────{STYLE_DIM:#}");

    // Fetch dependencies
    println!("  {STYLE_DIM}Fetching Splitwise groups and expenses...{STYLE_DIM:#}");
    let groups_res: GroupResponse = sw_client.fetch("get_groups", &[] as &[(&str, &str)]).await;
    let group_map: HashMap<u64, String> = groups_res
        .groups
        .iter()
        .map(|g| (g.id, g.name.clone()))
        .collect();

    let target_group = groups_res
        .groups
        .iter()
        .find(|g| g.id == sync_args.group_id);
    let group_name = target_group
        .map(|g| g.name.clone())
        .unwrap_or_else(|| "Unknown Group".to_string());

    println!(
        "{STYLE_INFO}👥 Group:{STYLE_INFO:#} {} (ID: {})",
        group_name, sync_args.group_id
    );
    if let Some(g) = target_group {
        let balance_str = format_group_balances(g, config.splitwise.user_id);
        println!("{STYLE_INFO}💰 Balance:{STYLE_INFO:#} {}", balance_str);
    }
    println!();

    let group_id_str = sync_args.group_id.to_string();
    let sw_query = [("group_id", group_id_str.as_str()), ("limit", "0")];
    let expenses_res: ExpensesResponse = sw_client.fetch("get_expenses", &sw_query).await;

    // Verify configured manual accounts exist in Lunch Money
    let accounts_res: ManualAccountsResponse = lm_client
        .fetch("manual_accounts", &[] as &[(&str, &str)])
        .await;
    for (currency, &account_id) in &config.lunch_money.target_accounts {
        if !accounts_res.manual_accounts.iter().any(|acc| acc.id == account_id) {
            eprintln!(
                "\n{STYLE_ERROR}❌ Error:{STYLE_ERROR:#} Configured manual account ID {} for currency '{}' has been deleted or does not exist in Lunch Money.\n\
                 Please check your Lunch Money manual accounts or run 'splitwise-lunchmoney init'.\n",
                account_id, currency
            );
            std::process::exit(1);
        }
    }

    let get_account_name = |manual_account_id: Option<u64>, currency: &str| -> String {
        let id = manual_account_id.or_else(|| {
            let currency_upper = currency.to_uppercase();
            config.lunch_money.target_accounts.get(&currency_upper).copied()
        });
        if let Some(id) = id {
            if let Some(acc) = accounts_res.manual_accounts.iter().find(|acc| acc.id == id) {
                return acc.display_name.as_deref().unwrap_or(&acc.name).to_string();
            }
        }
        "Unknown Account".to_string()
    };

    println!("  {STYLE_DIM}Fetching Lunch Money transactions...{STYLE_DIM:#}");
    let end_window_str = jiff::Timestamp::now()
        .to_zoned(jiff::tz::TimeZone::UTC)
        .strftime("%Y-%m-%d")
        .to_string();
    let mut lm_transactions = Vec::new();
    for &account_id in config.lunch_money.target_accounts.values() {
        let account_id_str = account_id.to_string();
        let lm_query = [
            ("start_date", "2000-01-01"),
            ("end_date", end_window_str.as_str()),
            ("manual_account_id", account_id_str.as_str()),
            ("limit", "1000"),
        ];
        let lm_res: TransactionsResponse = lm_client.fetch("transactions", &lm_query).await;
        let is_loan = accounts_res
            .manual_accounts
            .iter()
            .find(|acc| acc.id == account_id)
            .map(|acc| acc.account_type == "loan")
            .unwrap_or(false);

        let mut txs = lm_res.transactions;
        if is_loan {
            for t in &mut txs {
                t.amount = -t.amount;
            }
        }
        lm_transactions.extend(txs);
    }

    println!("  {STYLE_DIM}Comparing transactions...{STYLE_DIM:#}\n");

    let mut lm_map: HashMap<String, Transaction> = lm_transactions
        .into_iter()
        .filter_map(|t| t.external_id.clone().map(|ext_id| (ext_id, t)))
        .collect();

    // Prepare batch operations
    let mut inserts: Vec<InsertObject> = Vec::new();
    let mut updates: Vec<UpdateObject> = Vec::new();
    let mut deletes: Vec<Transaction> = Vec::new();

    for expense in expenses_res.expenses {
        let external_id = format!("splitwise_{}", expense.id);

        let net_balance = expense
            .users
            .iter()
            .find(|u| u.user_id == config.splitwise.user_id)
            .map(|u| u.net_balance)
            .unwrap_or(Decimal::ZERO);

        let is_ignored = expense
            .group_id
            .is_some_and(|gid| config.splitwise.ignored_groups.contains(&gid));

        // Skip ignored, deleted, or un-involved expenses
        if expense.deleted_at.is_some() || is_ignored || net_balance.is_zero() {
            if let Some(existing_lm) = lm_map.remove(&external_id) {
                deletes.push(existing_lm);
            }
            continue;
        }

        let currency_upper = expense.currency_code.to_uppercase();
        if !config
            .lunch_money
            .target_accounts
            .contains_key(&currency_upper)
        {
            eprintln!(
                "\n{STYLE_ERROR}❌ Error:{STYLE_ERROR:#} No manual account configured for currency '{}'.\n\
                 Please run 'splitwise-lunchmoney init' or set up 'Splitwise {}' manual account.\n",
                currency_upper, currency_upper
            );
            std::process::exit(1);
        }

        let date_civil = expense.date.to_zoned(jiff::tz::TimeZone::UTC).date();
        let currency_lower = expense.currency_code.to_lowercase();

        let payee_str = format!(
            "Splitwise - {}",
            match expense.group_id {
                Some(gid) => group_map
                    .get(&gid)
                    .cloned()
                    .unwrap_or_else(|| "Unknown Group".to_string()),
                None => expense
                    .users
                    .iter()
                    .find(|u| u.user_id != config.splitwise.user_id)
                    .and_then(|u| u.user.as_ref())
                    .map(|d| {
                        format!(
                            "{} {}",
                            d.first_name.as_deref().unwrap_or(""),
                            d.last_name.as_deref().unwrap_or("")
                        )
                        .trim()
                        .to_string()
                    })
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| "Non-group".to_string()),
            }
        );

        if let Some(existing_lm) = lm_map.remove(&external_id) {
            let amount_changed = existing_lm.amount != net_balance;
            let notes_changed = existing_lm.notes.as_deref().unwrap_or("") != expense.description;

            if amount_changed
                || existing_lm.date != date_civil
                || existing_lm.currency != currency_lower
                || existing_lm.payee != payee_str
                || notes_changed
            {
                updates.push(UpdateObject {
                    id: existing_lm.id,
                    date: date_civil,
                    amount: net_balance,
                    currency: currency_lower,
                    payee: payee_str,
                    notes: expense.description,
                });
            }
        } else {
            let manual_account_id = config.lunch_money.target_accounts[&currency_upper];
            inserts.push(InsertObject {
                date: date_civil,
                amount: net_balance,
                currency: currency_lower,
                payee: payee_str,
                notes: expense.description,
                external_id,
                manual_account_id,
                status: "reviewed".to_string(),
            });
        }
    }

    // Filter deletes to only target transactions belonging to this specific group
    let is_non_group = sync_args.group_id == 0;
    let group_payee = format!("Splitwise - {}", group_name);

    for (_ext_id, t) in lm_map {
        let belongs_to_group = if is_non_group {
            t.payee == "Splitwise - Non-group"
                || (!group_map
                    .values()
                    .any(|gn| t.payee == format!("Splitwise - {}", gn))
                    && t.payee.starts_with("Splitwise - "))
        } else {
            t.payee == group_payee
        };

        if belongs_to_group {
            deletes.push(t);
        }
    }

    // Execute batches
    if !deletes.is_empty() {
        println!(
            "🗑️  {STYLE_WARNING}Deleting {STYLE_WARNING:#}{} old/modified transaction(s) from Lunch Money:",
            deletes.len()
        );
        for t in &deletes {
            let acc_name = get_account_name(t.manual_account_id, &t.currency);
            println!(
                "   {STYLE_ERROR}-{STYLE_ERROR:#} {}",
                format_transaction_summary(&t.payee, t.amount, &t.currency, t.date, t.notes.as_deref().unwrap_or(""), &acc_name)
            );
        }
        println!();

        if !sync_args.dry_run {
            let delete_ids: Vec<u64> = deletes.iter().map(|t| t.id).collect();
            lm_client
                .exec(
                    Method::DELETE,
                    "transactions",
                    &DeletePayload { ids: delete_ids },
                )
                .await;
        }
    }

    if !updates.is_empty() {
        println!(
            "✎  {STYLE_INFO}Updating {STYLE_INFO:#}{} modified transaction(s) in Lunch Money:",
            updates.len()
        );
        for u in &updates {
            let acc_name = get_account_name(None, &u.currency);
            println!(
                "   {STYLE_INFO}~{STYLE_INFO:#} {}",
                format_transaction_summary(&u.payee, u.amount, &u.currency, u.date, &u.notes, &acc_name)
            );
        }
        println!();

        if !sync_args.dry_run {
            for chunk in updates.chunks(500) {
                let mut chunk_txs = chunk.to_vec();
                for u in &mut chunk_txs {
                    let is_loan = accounts_res
                        .manual_accounts
                        .iter()
                        .find(|acc| {
                            let curr = u.currency.to_uppercase();
                            config.lunch_money.target_accounts.get(&curr).copied() == Some(acc.id)
                        })
                        .map(|acc| acc.account_type == "loan")
                        .unwrap_or(false);
                    if is_loan {
                        u.amount = -u.amount;
                    }
                }
                lm_client
                    .exec(
                        Method::PUT,
                        "transactions",
                        &UpdatePayload {
                            transactions: chunk_txs,
                        },
                    )
                    .await;
            }
        }
    }

    if !inserts.is_empty() {
        println!(
            "✓  {STYLE_SUCCESS}Inserting {STYLE_SUCCESS:#}{} new transaction(s) to Lunch Money:",
            inserts.len()
        );
        for ins in &inserts {
            let acc_name = get_account_name(Some(ins.manual_account_id), &ins.currency);
            println!(
                "   {STYLE_SUCCESS}+{STYLE_SUCCESS:#} {}",
                format_transaction_summary(&ins.payee, ins.amount, &ins.currency, ins.date, &ins.notes, &acc_name)
            );
        }
        println!();

        if !sync_args.dry_run {
            for chunk in inserts.chunks(500) {
                let mut chunk_txs = chunk.to_vec();
                for ins in &mut chunk_txs {
                    let is_loan = accounts_res
                        .manual_accounts
                        .iter()
                        .find(|acc| acc.id == ins.manual_account_id)
                        .map(|acc| acc.account_type == "loan")
                        .unwrap_or(false);
                    if is_loan {
                        ins.amount = -ins.amount;
                    }
                }
                lm_client
                    .exec(
                        Method::POST,
                        "transactions",
                        &InsertPayload {
                            transactions: chunk_txs,
                        },
                    )
                    .await;
            }
        }
    }

    if deletes.is_empty() && updates.is_empty() && inserts.is_empty() {
        println!(
            "{STYLE_SUCCESS}✨ No changes detected. Lunch Money manual account is up-to-date!{STYLE_SUCCESS:#}\n"
        );
    } else if sync_args.dry_run {
        println!(
            "{STYLE_WARNING}⚠️ Dry run complete! No changes were made to Lunch Money.{STYLE_WARNING:#}\n"
        );
    } else {
        println!("{STYLE_SUCCESS}✨ Synchronization cycle complete!{STYLE_SUCCESS:#}\n");
    }
}

fn format_group_balances(group: &api::splitwise::schema::Group, user_id: u64) -> String {
    let mut parts = Vec::new();
    if let Some(members) = &group.members {
        if let Some(member) = members.iter().find(|m| m.id == user_id) {
            for bal in &member.balance {
                let amount = bal.amount;
                let currency = &bal.currency_code;
                let amount_str = format!("{:.2} {}", amount, currency);
                let styled = if amount.is_sign_negative() {
                    format!(
                        "{}{}{}",
                        STYLE_ERROR,
                        amount_str,
                        STYLE_ERROR.render_reset()
                    )
                } else if amount.is_zero() {
                    format!("{}{}{}", STYLE_DIM, amount_str, STYLE_DIM.render_reset())
                } else {
                    format!(
                        "{}{}{}",
                        STYLE_SUCCESS,
                        amount_str,
                        STYLE_SUCCESS.render_reset()
                    )
                };
                parts.push(styled);
            }
        }
    }
    if parts.is_empty() {
        format!("{}—{}", STYLE_DIM, STYLE_DIM.render_reset())
    } else {
        parts.join(", ")
    }
}
