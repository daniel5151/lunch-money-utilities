pub mod style;

mod api;
mod cli;
mod commands;
mod config;

#[tokio::main]
async fn main() {
    use clap::Parser;

    let args = cli::Args::parse();

    match args.command {
        cli::Commands::Init => {
            commands::init::run_init().await;
        }
        cli::Commands::Sync(sync_args) => match sync_args.command {
            cli::SyncSubcommands::Window(args) => {
                commands::sync::run_sync_window(args).await;
            }
            cli::SyncSubcommands::Group(args) => {
                commands::sync::run_sync_group(args).await;
            }
            cli::SyncSubcommands::Balances(args) => {
                commands::sync_balances::run_sync_balances(args).await;
            }
        },
        cli::Commands::Query(query_args) => match query_args.command {
            cli::QuerySubcommands::Splitwise(splitwise_args) => match splitwise_args.command {
                cli::QuerySplitwiseSubcommands::Window(args) => {
                    commands::query::run_query_splitwise_window(args).await;
                }
                cli::QuerySplitwiseSubcommands::Group(args) => {
                    commands::query::run_query_splitwise_group(args).await;
                }
                cli::QuerySplitwiseSubcommands::GetGroups => {
                    commands::query::run_query_splitwise_get_groups().await;
                }
            },
            cli::QuerySubcommands::LunchMoney(lunchmoney_args) => match lunchmoney_args.command {
                cli::QueryLunchMoneySubcommands::Categories => {
                    commands::query::run_query_lunchmoney_categories().await;
                }
            },
        },
    }
}

pub fn load_config() -> config::Config {
    let filename = "splitwise-lunchmoney.toml";

    // 1. Check current working directory
    let path = std::path::Path::new(filename);
    if path.exists() {
        let content = std::fs::read_to_string(path)
            .expect("Failed to read splitwise-lunchmoney.toml from current working directory");
        return toml::from_str(&content).expect("Malformed splitwise-lunchmoney.toml file");
    }

    // 2. Check directory of the running executable
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let candidate = exe_dir.join(filename);
            if candidate.exists() {
                let content = std::fs::read_to_string(&candidate)
                    .expect("Failed to read splitwise-lunchmoney.toml from executable directory");
                return toml::from_str(&content).expect("Malformed splitwise-lunchmoney.toml file");
            }
        }
    }

    use crate::style::STYLE_ERROR;

    anstream::eprintln! {};
    anstream::eprintln! { "{STYLE_ERROR}❌ Error:{STYLE_ERROR:#} Configuration file 'splitwise-lunchmoney.toml' not found in current directory or executable directory." };
    anstream::eprintln! { "Please run 'splitwise-lunchmoney init' to configure." };
    anstream::eprintln! {};
    std::process::exit(1);
}
