mod cli;
mod commands;
mod config;

pub use cli::Cli;

use lm_common::style;
use lm_common::tool::Tool;
use lm_common::tool::ToolContext;

/// The Venmo balance-fixer tool.
pub struct VenmoTool;

impl Tool for VenmoTool {
    const NAME: &'static str = "venmo-balfixer";
    type Cli = Cli;

    async fn run(cx: &ToolContext, cli: Cli) -> anyhow::Result<()> {
        match cli.command {
            cli::Commands::Init(init_args) => {
                commands::init::run_init(init_args).await?;
            }
            cli::Commands::Reconcile(reconcile_args) => {
                let (doc, _path) = lm_common::config::load_document()?;
                let common = lm_common::config::common_section(&doc)?;
                let config: config::Config = lm_common::config::deserialize_section(&doc, "venmo")?;
                let lm_api_key = common.lm_api_key.clone().ok_or_else(|| {
                    anyhow::anyhow!(
                        "Missing [common].lm_api_key in lm_utils.toml. Run \
                         `lm-utils venmo-balfixer init` to configure it."
                    )
                })?;
                commands::reconcile::run_reconcile(
                    cx,
                    &config,
                    &lm_api_key,
                    common.retry,
                    reconcile_args,
                )
                .await?;
            }
        }
        Ok(())
    }
}
