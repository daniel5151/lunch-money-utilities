mod cli;
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
    type Cli = Cli;

    async fn run(cx: &ToolContext, cli: Cli) -> anyhow::Result<()> {
        match cli.command {
            cli::Commands::Init(init_args) => {
                commands::init::run_init(init_args).await?;
            }
            cli::Commands::Import(import_args) => {
                let (doc, _path) = lm_common::config::load_document()?;
                let common = lm_common::config::common_section(&doc)?;
                let config: config::Config =
                    lm_common::config::deserialize_section(&doc, "payslip")?;
                config.validate()?;
                commands::import::run_import(
                    cx,
                    config,
                    common.lm_api_key,
                    common.retry,
                    import_args,
                )
                .await?;
            }
        }
        Ok(())
    }
}
