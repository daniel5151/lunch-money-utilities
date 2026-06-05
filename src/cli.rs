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
    Init,
    /// Query data from Splitwise or Lunch Money
    Query(QueryArgs),
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
    /// Query Splitwise expenses for a specific group
    Group(QuerySplitwiseGroupArgs),
    /// List all Splitwise groups you belong to, including their ID, name, last updated date, and outstanding balances
    #[command(name = "get-groups")]
    GetGroups,
    /// List all Splitwise transaction categories (parent categories and their subcategories)
    Categories,
}

#[derive(Parser, Debug)]
pub struct QuerySplitwiseWindowArgs {
    /// Window duration for querying (e.g., "3 days", "24h", "1 week")
    #[arg(value_parser = humantime::parse_duration)]
    pub window: std::time::Duration,
}

#[derive(Parser, Debug)]
pub struct QuerySplitwiseGroupArgs {
    /// The Splitwise Group ID to filter by
    pub group_id: u64,
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
}

#[derive(Parser, Debug)]
pub struct SyncWindowArgs {
    /// Window duration for synchronization (e.g., "3 days", "24h", "1 week")
    #[arg(value_parser = humantime::parse_duration)]
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

    /// Optional tag to associate with imported transactions in Lunch Money
    #[arg(long)]
    pub tag: Option<String>,

    /// Bypass the check for ignored groups
    #[arg(long)]
    pub bypass_ignore: bool,
}
