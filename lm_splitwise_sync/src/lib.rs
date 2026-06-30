mod api;
pub mod cli;
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
    const CONFIG_SECTION: &'static str = "splitwise";
    type Cli = Cli;
    type Config = config::Config;

    async fn run(
        cx: &ToolContext,
        cli: Cli,
        config_path: std::path::PathBuf,
        common_config: lm_common::config::CommonConfig,
        tool_config: Option<Self::Config>,
    ) -> anyhow::Result<()> {
        match cli.command {
            cli::Commands::Init(init_args) => {
                commands::init::run_init(init_args, config_path).await?;
            }
            cmd => {
                let config = tool_config.ok_or_else(|| {
                    anyhow::anyhow!(
                        "Missing [splitwise] section in lm_utils.toml. Run \
                         `lm-utils splitwise-sync init` to configure it."
                    )
                })?;
                let lm_api_key = common_config.lm_api_key.clone().ok_or_else(|| {
                    anyhow::anyhow!(
                        "Missing [common].lm_api_key in lm_utils.toml. Run \
                         `lm-utils splitwise-sync init` to configure it."
                    )
                })?;
                let splitwise =
                    api::splitwise::Client::new(cx.http.clone(), config.splitwise.api_key.clone());
                let lunch_money = api::lunch_money::Client::new(
                    cx.http.clone(),
                    lm_api_key,
                    common_config.retry.into(),
                );
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
                        cli::SyncSubcommands::Person(args) => {
                            commands::sync::run_sync_person(&ctx, args).await?;
                        }
                        cli::SyncSubcommands::Balances(args) => {
                            commands::sync_balances::run_sync_balances(&ctx, args).await?;
                        }
                    },
                    cli::Commands::Query(query_args) => match query_args.command {
                        cli::QuerySubcommands::Window(args) => {
                            commands::query::run_query_splitwise_window(&ctx, args).await?;
                        }
                        cli::QuerySubcommands::WindowUpdates(args) => {
                            commands::query::run_query_splitwise_window_updates(&ctx, args).await?;
                        }
                        cli::QuerySubcommands::Group(args) => {
                            commands::query::run_query_splitwise_group(&ctx, args).await?;
                        }
                        cli::QuerySubcommands::Groups => {
                            commands::query::run_query_splitwise_groups(&ctx).await?;
                        }
                        cli::QuerySubcommands::Categories => {
                            commands::query::run_query_splitwise_categories(&ctx).await?;
                        }
                        cli::QuerySubcommands::AccountMap => {
                            commands::query::run_query_account_map(&ctx).await?;
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
