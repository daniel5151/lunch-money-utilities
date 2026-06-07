use anstream::eprintln;

pub mod style;

mod api;
mod cli;
mod commands;
mod config;

#[tokio::main]
async fn main() {
    if let Err(err) = run().await {
        use crate::style::STYLE_ERROR;
        eprintln! {};
        eprintln! { "{STYLE_ERROR}❌ Error:{STYLE_ERROR:#} {err}" };

        let mut causes = err.chain().skip(1).peekable();
        if causes.peek().is_some() {
            eprintln! {};
            eprintln! { "Caused by:" };
            for cause in causes {
                eprintln! { "  • {cause}" };
            }
        }
        eprintln! {};
        std::process::exit(1);
    }
}

async fn run() -> anyhow::Result<()> {
    use clap::Parser;

    let args = cli::Args::parse();

    match args.command {
        cli::Commands::Init => {
            commands::init::run_init().await?;
        }
        cli::Commands::Sync(sync_args) => match sync_args.command {
            cli::SyncSubcommands::Window(args) => {
                commands::sync::run_sync_window(args).await?;
            }
            cli::SyncSubcommands::Group(args) => {
                commands::sync::run_sync_group(args).await?;
            }
            cli::SyncSubcommands::Balances(args) => {
                commands::sync_balances::run_sync_balances(args).await?;
            }
        },
        cli::Commands::Query(query_args) => match query_args.command {
            cli::QuerySubcommands::Splitwise(splitwise_args) => match splitwise_args.command {
                cli::QuerySplitwiseSubcommands::Window(args) => {
                    commands::query::run_query_splitwise_window(args).await?;
                }
                cli::QuerySplitwiseSubcommands::Group(args) => {
                    commands::query::run_query_splitwise_group(args).await?;
                }
                cli::QuerySplitwiseSubcommands::Groups => {
                    commands::query::run_query_splitwise_groups().await?;
                }
                cli::QuerySplitwiseSubcommands::Categories => {
                    commands::query::run_query_splitwise_categories().await?;
                }
            },
            cli::QuerySubcommands::LunchMoney(lunchmoney_args) => match lunchmoney_args.command {
                cli::QueryLunchMoneySubcommands::Categories => {
                    commands::query::run_query_lunchmoney_categories().await?;
                }
                cli::QueryLunchMoneySubcommands::Tags => {
                    commands::query::run_query_lunchmoney_tags().await?;
                }
                cli::QueryLunchMoneySubcommands::Accounts => {
                    commands::query::run_query_lunchmoney_accounts().await?;
                }
            },
        },
    }
    Ok(())
}

pub fn load_config() -> anyhow::Result<config::Config> {
    use anyhow::Context;
    let filename = "splitwise-lunchmoney.toml";

    // 1. Check current working directory
    let path = std::path::Path::new(filename);
    if path.exists() {
        let content = std::fs::read_to_string(path)
            .context("Failed to read splitwise-lunchmoney.toml from current working directory")?;
        let config =
            toml::from_str(&content).context("Malformed splitwise-lunchmoney.toml file")?;
        return Ok(config);
    }

    // 2. Check directory of the running executable
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let candidate = exe_dir.join(filename);
            if candidate.exists() {
                let content = std::fs::read_to_string(&candidate).context(
                    "Failed to read splitwise-lunchmoney.toml from executable directory",
                )?;
                let config =
                    toml::from_str(&content).context("Malformed splitwise-lunchmoney.toml file")?;
                return Ok(config);
            }
        }
    }

    anyhow::bail!(
        "Configuration file 'splitwise-lunchmoney.toml' not found in current directory or executable directory.\n\
        Please run 'splitwise-lunchmoney init' to configure."
    )
}
