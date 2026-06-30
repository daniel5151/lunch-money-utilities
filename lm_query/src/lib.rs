pub mod cli;
mod commands;

pub use cli::Cli;
use lm_common::tool::Tool;
use lm_common::tool::ToolContext;

/// The Lunch Money query tool.
pub struct QueryTool;

#[derive(Debug, serde::Deserialize)]
pub struct Config {}

impl Tool for QueryTool {
    const NAME: &'static str = "query";
    const CONFIG_SECTION: &'static str = "query";
    type Cli = Cli;
    type Config = Config;

    async fn run(
        cx: &ToolContext,
        cli: Cli,
        _config_path: std::path::PathBuf,
        common_config: lm_common::config::CommonConfig,
        _tool_config: Option<Self::Config>,
    ) -> anyhow::Result<()> {
        let lm_api_key = common_config.lm_api_key.ok_or_else(|| {
            anyhow::anyhow!("Missing [common].lm_api_key in lm_utils.toml.")
        })?;
        let lm_client = lunch_money::client::Client::new(
            cx.http.clone(),
            lm_api_key,
            common_config.retry.into(),
        );

        match cli.command {
            cli::Commands::Categories => {
                commands::run_query_categories(&lm_client).await?;
            }
            cli::Commands::Tags => {
                commands::run_query_tags(&lm_client).await?;
            }
            cli::Commands::Accounts => {
                commands::run_query_accounts(&lm_client).await?;
            }
        }
        Ok(())
    }
}
