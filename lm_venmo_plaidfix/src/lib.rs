pub mod cli;
mod commands;
mod config;

pub use cli::Cli;
use lm_common::style;
use lm_common::tool::Tool;
use lm_common::tool::ToolContext;

/// The Venmo plaid-fixer tool.
pub struct VenmoTool;

impl Tool for VenmoTool {
    const NAME: &'static str = "venmo-plaidfix";
    const CONFIG_SECTION: &'static str = "venmo";
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
            cli::Commands::Reconcile(reconcile_args) => {
                let config = tool_config.ok_or_else(|| {
                    anyhow::anyhow!(
                        "Missing [venmo] section in lm_utils.toml. Run \
                         `lm-utils venmo-plaidfix init` to configure it."
                    )
                })?;
                let lm_api_key = common_config.lm_api_key.clone().ok_or_else(|| {
                    anyhow::anyhow!(
                        "Missing [common].lm_api_key in lm_utils.toml. Run \
                         `lm-utils venmo-plaidfix init` to configure it."
                    )
                })?;
                commands::reconcile::run_reconcile(
                    cx,
                    &config,
                    &lm_api_key,
                    common_config.retry,
                    reconcile_args,
                )
                .await?;
            }
        }
        Ok(())
    }
}
