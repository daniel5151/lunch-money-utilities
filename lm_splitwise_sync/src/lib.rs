mod api;
mod cli;
mod commands;
mod config;
mod metadata;

pub use cli::Cli;

use lm_common::style;
use lm_common::tool::Tool;
use lm_common::tool::ToolContext;

/// Per-invocation context for the Splitwise tool's command handlers.
///
/// This is the tool-local generalization of the former standalone `AppContext`.
/// The cross-tool shared services (`http`, `dry_run`) come from the
/// [`ToolContext`]; the tool-specific clients (`splitwise`, `lunch_money`) and
/// the parsed config are built inside [`SplitwiseTool::run`] and live here.
pub struct AppContext {
    pub config: config::Config,
    pub http: reqwest::Client,
    pub dry_run: bool,
    pub splitwise: api::splitwise::Client,
    pub lunch_money: api::lunch_money::Client,
}

/// The Splitwise <-> Lunch Money sync tool.
pub struct SplitwiseTool;

impl Tool for SplitwiseTool {
    const NAME: &'static str = "splitwise-sync";
    type Cli = Cli;

    async fn run(cx: &ToolContext, cli: Cli) -> anyhow::Result<()> {
        match cli.command {
            cli::Commands::Init(init_args) => {
                commands::init::run_init(init_args).await?;
            }
            cmd => {
                let (doc, _path) = lm_common::config::load_document()?;
                let common = lm_common::config::common_section(&doc)?;
                let config: config::Config =
                    lm_common::config::deserialize_section(&doc, "splitwise")?;
                let lm_api_key = common.lm_api_key.clone().ok_or_else(|| {
                    anyhow::anyhow!(
                        "Missing [common].lm_api_key in lm_utils.toml. Run \
                         `lm-utils splitwise-sync init` to configure it."
                    )
                })?;
                let splitwise = api::splitwise::Client::new(
                    cx.http.clone(),
                    config.splitwise.api_key.clone(),
                );
                let lunch_money = api::lunch_money::Client::new(cx.http.clone(), lm_api_key);
                let ctx = AppContext {
                    config,
                    http: cx.http.clone(),
                    dry_run: cx.dry_run,
                    splitwise,
                    lunch_money,
                };

                match cmd {
                    cli::Commands::Init(_) => unreachable!(),
                    cli::Commands::Sync(sync_args) => match sync_args.command {
                        cli::SyncSubcommands::Window(args) => {
                            commands::sync::run_sync_window(&ctx, args).await?;
                        }
                        cli::SyncSubcommands::Group(args) => {
                            commands::sync::run_sync_group(&ctx, args).await?;
                        }
                        cli::SyncSubcommands::Balances(args) => {
                            commands::sync_balances::run_sync_balances(&ctx, args).await?;
                        }
                    },
                    cli::Commands::Query(query_args) => match query_args.command {
                        cli::QuerySubcommands::Splitwise(splitwise_args) => {
                            match splitwise_args.command {
                                cli::QuerySplitwiseSubcommands::Window(args) => {
                                    commands::query::run_query_splitwise_window(&ctx, args).await?;
                                }
                                cli::QuerySplitwiseSubcommands::WindowUpdates(args) => {
                                    commands::query::run_query_splitwise_window_updates(&ctx, args)
                                        .await?;
                                }
                                cli::QuerySplitwiseSubcommands::Group(args) => {
                                    commands::query::run_query_splitwise_group(&ctx, args).await?;
                                }
                                cli::QuerySplitwiseSubcommands::Groups => {
                                    commands::query::run_query_splitwise_groups(&ctx).await?;
                                }
                                cli::QuerySplitwiseSubcommands::Categories => {
                                    commands::query::run_query_splitwise_categories(&ctx).await?;
                                }
                            }
                        }
                        cli::QuerySubcommands::LunchMoney(lunchmoney_args) => {
                            match lunchmoney_args.command {
                                cli::QueryLunchMoneySubcommands::Categories => {
                                    commands::query::run_query_lunchmoney_categories(&ctx).await?;
                                }
                                cli::QueryLunchMoneySubcommands::Tags => {
                                    commands::query::run_query_lunchmoney_tags(&ctx).await?;
                                }
                                cli::QueryLunchMoneySubcommands::Accounts => {
                                    commands::query::run_query_lunchmoney_accounts(&ctx).await?;
                                }
                            }
                        }
                    },
                    cli::Commands::Migrate(migrate_args) => match migrate_args.command {
                        cli::MigrateSubcommands::AddMetadata(args) => {
                            commands::migrate::run_migrate_add_metadata(&ctx, args).await?;
                        }
                    },
                }
            }
        }
        Ok(())
    }
}

