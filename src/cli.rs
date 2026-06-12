use clap::Parser;
use clap::Subcommand;

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

/// Synchronize Splitwise transactions and global outstanding balances into Lunch Money manual accounts
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None, styles = cli_styles())]
pub struct Args {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Sync Splitwise transactions or global balances to Lunch Money
    Sync(SyncArgs),
    /// Run the interactive setup wizard to configure splitwise-lunchmoney.toml
    Init(InitArgs),
    /// Query data from Splitwise or Lunch Money
    Query(QueryArgs),
    /// Migrate previously imported transactions in Lunch Money
    Migrate(MigrateArgs),
}

#[derive(Parser, Debug)]
pub struct InitArgs {
    /// Output file path (defaults to splitwise-lunchmoney.toml)
    #[arg(long, hide = true)]
    pub file: Option<std::path::PathBuf>,
}

#[derive(Parser, Debug)]
pub struct QueryArgs {
    #[command(subcommand)]
    pub command: QuerySubcommands,
}

#[derive(Subcommand, Debug)]
pub enum QuerySubcommands {
    /// Query Splitwise expenses
    Splitwise(QuerySplitwiseArgs),
    /// Query Lunch Money data
    #[command(name = "lunchmoney")]
    LunchMoney(QueryLunchMoneyArgs),
}

#[derive(Parser, Debug)]
pub struct QueryLunchMoneyArgs {
    #[command(subcommand)]
    pub command: QueryLunchMoneySubcommands,
}

#[derive(Subcommand, Debug)]
pub enum QueryLunchMoneySubcommands {
    /// List what categories the user has set up in Lunch Money
    Categories,
    /// List all tags in Lunch Money
    Tags,
    /// List all manual accounts in Lunch Money
    Accounts,
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
    /// Query Splitwise expenses updated in a given time window, alongside their update events
    #[command(name = "window-updates")]
    WindowUpdates(QuerySplitwiseWindowUpdatesArgs),
    /// Query Splitwise expenses for a specific group
    Group(QuerySplitwiseGroupArgs),
    /// List all Splitwise groups you belong to, including their ID, name, last updated date, and outstanding balances
    Groups,
    /// List all Splitwise transaction categories (parent categories and their subcategories)
    Categories,
}

#[derive(Parser, Debug)]
pub struct QuerySplitwiseWindowUpdatesArgs {
    /// Window duration for querying (e.g., "3 days", "24h", "1 week")
    #[arg(value_parser = humantime::parse_duration)]
    pub window: std::time::Duration,

    /// Optional date to offset the window from (YYYY-MM-DD, defaults to today's date)
    #[arg(long)]
    pub from: Option<jiff::civil::Date>,
}

#[derive(Parser, Debug)]
pub struct QuerySplitwiseWindowArgs {
    /// Window duration for querying (e.g., "3 days", "24h", "1 week")
    #[arg(value_parser = humantime::parse_duration)]
    pub window: std::time::Duration,

    /// Optional date to offset the window from (YYYY-MM-DD, defaults to today's date)
    #[arg(long)]
    pub from: Option<jiff::civil::Date>,

    /// Only include non-group transactions (i.e. between individuals, outside a group)
    #[arg(long)]
    pub no_groups: bool,
}

#[derive(Parser, Debug)]
pub struct QuerySplitwiseGroupArgs {
    /// The Splitwise Group ID or name to filter by
    pub group: String,
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
    /// Sync user's global Splitwise balances into Lunch Money's manual accounts
    Balances(SyncBalancesArgs),
}

#[derive(Parser, Debug)]
pub struct SyncBalancesArgs {
    /// Print what would be synced without modifying Lunch Money
    #[arg(long)]
    pub dry_run: bool,

    /// Path to a new CSV file to dump the sync operations (defaults to balances.csv if omitted)
    #[arg(long, num_args = 0..=1)]
    #[expect(clippy::option_option)]
    pub csv: Option<Option<std::path::PathBuf>>,

    /// Skip the configured loan_tag in config toml
    #[arg(long)]
    pub no_loan_tag: bool,
}

#[derive(Parser, Debug)]
pub struct SyncWindowArgs {
    /// Window duration for synchronization (e.g., "3 days", "24h", "1 week")
    #[arg(value_parser = humantime::parse_duration)]
    pub window: std::time::Duration,

    /// Optional date to offset the window from (YYYY-MM-DD, defaults to today's date)
    #[arg(long)]
    pub from: Option<jiff::civil::Date>,

    /// Exclude transactions newer than this grace period duration (e.g., "1h", "15m", "2 hours")
    #[arg(long, value_parser = humantime::parse_duration)]
    pub grace_period: Option<std::time::Duration>,

    /// Print what would be synced without modifying Lunch Money
    #[arg(long)]
    pub dry_run: bool,

    /// Optional tag to associate with imported transactions in Lunch Money
    #[arg(long)]
    pub tag: Option<String>,

    /// Only include non-group transactions (i.e. between individuals, outside a group)
    #[arg(long)]
    pub no_groups: bool,

    /// Path to a new CSV file to dump the sync operations
    #[arg(long)]
    pub csv: Option<std::path::PathBuf>,

    /// Skip the configured loan_tag in config toml
    #[arg(long)]
    pub no_loan_tag: bool,

    /// Bypass the check for ignored groups
    #[arg(long)]
    pub no_ignore: bool,
}

#[derive(Parser, Debug)]
pub struct SyncGroupArgs {
    /// The Splitwise Group ID or name to synchronize
    pub group: String,

    /// Print what would be synced without modifying Lunch Money
    #[arg(long)]
    pub dry_run: bool,

    /// Optional tag to associate with imported transactions in Lunch Money
    #[arg(long)]
    pub tag: Option<String>,

    /// Force all transactions to get mapped to this Lunch Money category (ID or name)
    #[arg(long)]
    pub force_category: Option<String>,

    /// Bypass the check for ignored groups
    #[arg(long)]
    pub no_ignore: bool,

    /// Path to a new CSV file to dump the sync operations (defaults to <group_name>.csv if omitted)
    #[arg(long, num_args = 0..=1)]
    #[expect(clippy::option_option)]
    pub csv: Option<Option<std::path::PathBuf>>,

    /// Skip the configured loan_tag in config toml
    #[arg(long)]
    pub no_loan_tag: bool,
}

#[derive(Parser, Debug)]
pub struct MigrateArgs {
    #[command(subcommand)]
    pub command: MigrateSubcommands,
}

#[derive(Subcommand, Debug)]
pub enum MigrateSubcommands {
    /// Retroactively adds missing Splitwise metadata to existing Lunch Money transactions
    #[command(name = "add-metadata")]
    AddMetadata(MigrateAddMetadataArgs),
}

#[derive(Parser, Debug)]
pub struct MigrateAddMetadataArgs {
    /// Optional start date (YYYY-MM-DD) to scan from (defaults to 2000-01-01)
    #[arg(long)]
    pub start_date: Option<jiff::civil::Date>,

    /// Optional end date (YYYY-MM-DD) to scan to (defaults to today)
    #[arg(long)]
    pub end_date: Option<jiff::civil::Date>,

    /// Print what would be migrated without modifying Lunch Money
    #[arg(long)]
    pub dry_run: bool,
}
