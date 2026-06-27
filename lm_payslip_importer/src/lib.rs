pub mod cli;
mod commands;
mod config;
mod payslip;

pub use cli::Cli;
use lm_common::style;
use lm_common::tool::Tool;
use lm_common::tool::ToolContext;

/// The payslip importer tool.
pub struct PayslipTool;

impl Tool for PayslipTool {
    const NAME: &'static str = "payslip-importer";
    const CONFIG_SECTION: &'static str = "payslip";
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
            cli::Commands::Import(import_args) => {
                let config = tool_config.ok_or_else(|| {
                    anyhow::anyhow!(
                        "Missing [payslip] section in lm_utils.toml. Run `lm-utils payslip-importer init` to configure it."
                    )
                })?;
                config.validate()?;
                commands::import::run_import(
                    cx,
                    config,
                    common_config.lm_api_key,
                    common_config.retry,
                    import_args,
                )
                .await?;
            }
        }
        Ok(())
    }
}
