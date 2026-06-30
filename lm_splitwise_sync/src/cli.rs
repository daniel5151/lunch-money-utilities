use clap::Args;
use clap::Parser;
use clap::Subcommand;

/// Synchronize Splitwise transactions and global outstanding balances into Lunch Money manual accounts
#[derive(Args, Debug)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Sync Splitwise transactions or global balances to Lunch Money
    Sync(SyncArgs),
    /// Run the interactive setup wizard to configure lm_utils.toml
    Init(InitArgs),
    /// Query data from Splitwise
    Query(QueryArgs),
    /// Migrate previously imported transactions in Lunch Money
    Migrate(MigrateArgs),
}

#[derive(Args, Debug)]
pub struct InitArgs {
    /// Skip interactive logic and just print the LLM prompt for categorizing sections
    #[arg(long)]
    pub just_categorize: bool,
}

#[derive(Parser, Debug)]
pub struct QueryArgs {
    #[command(subcommand)]
    pub command: QuerySubcommands,
}

#[derive(Subcommand, Debug)]
pub enum QuerySubcommands {
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
    /// Show Lunch Money manual accounts with their Splitwise currency mappings
    #[command(name = "account-map")]
    AccountMap,
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
    /// Sync all transactions corresponding to a specific Splitwise person
    Person(SyncPersonArgs),
    /// Sync user's global Splitwise balances into Lunch Money's manual accounts
    Balances(SyncBalancesArgs),
}

#[derive(Parser, Debug)]
pub struct SyncBalancesArgs {
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
pub struct SyncPersonArgs {
    /// The Splitwise Person ID, email, or name to synchronize
    pub person: String,

    /// Optional date to offset the sync from (YYYY-MM-DD, defaults to today's date)
    #[arg(long)]
    pub from: Option<jiff::civil::Date>,

    /// Optional tag to associate with imported transactions in Lunch Money
    #[arg(long)]
    pub tag: Option<String>,

    /// Force all transactions to get mapped to this Lunch Money category (ID or name)
    #[arg(long)]
    pub force_category: Option<String>,

    /// Bypass the check for ignored groups
    #[arg(long)]
    pub no_ignore: bool,

    /// Path to a new CSV file to dump the sync operations (defaults to <person_name>.csv if omitted)
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
}
