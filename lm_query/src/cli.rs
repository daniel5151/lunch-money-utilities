use clap::Args;
use clap::Subcommand;

/// Query Lunch Money data
#[derive(Args, Debug)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// List categories in Lunch Money
    Categories,
    /// List tags in Lunch Money
    Tags,
    /// List manual accounts in Lunch Money
    Accounts,
}
