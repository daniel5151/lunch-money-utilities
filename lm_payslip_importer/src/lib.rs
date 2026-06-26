use anstream::eprintln;

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
                let config = load_config()?;
                commands::import::run_import(cx, config, import_args).await?;
            }
        }
        Ok(())
    }
}

fn load_config() -> anyhow::Result<config::Config> {
    use crate::style::STYLE_WARNING;
    use anyhow::Context;

    // 1. Check current working directory for new filename
    let new_path = std::path::Path::new("lm_payslip_importer.toml");
    if new_path.exists() {
        let content = std::fs::read_to_string(new_path)
            .context("Failed to read lm_payslip_importer.toml from current working directory")?;
        let config = config::Config::from_toml_str(&content)
            .context("Malformed lm_payslip_importer.toml file")?;
        return Ok(config);
    }

    // 2. Check current working directory for fallback filename
    let fallback_path = std::path::Path::new("workday_payslip_splitter.toml");
    if fallback_path.exists() {
        eprintln! { "{STYLE_WARNING}⚠️  Warning: Using deprecated configuration file 'workday_payslip_splitter.toml'. Please rename it to 'lm_payslip_importer.toml'.{STYLE_WARNING:#}" };
        let content = std::fs::read_to_string(fallback_path).context(
            "Failed to read workday_payslip_splitter.toml from current working directory",
        )?;
        let config = config::Config::from_toml_str(&content)
            .context("Malformed workday_payslip_splitter.toml file")?;
        return Ok(config);
    }

    // 3. Check directory of the running executable
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let candidate = exe_dir.join("lm_payslip_importer.toml");
            if candidate.exists() {
                let content = std::fs::read_to_string(&candidate)
                    .context("Failed to read lm_payslip_importer.toml from executable directory")?;
                let config = config::Config::from_toml_str(&content)
                    .context("Malformed lm_payslip_importer.toml file")?;
                return Ok(config);
            }

            let fallback_candidate = exe_dir.join("workday_payslip_splitter.toml");
            if fallback_candidate.exists() {
                eprintln! { "{STYLE_WARNING}⚠️  Warning: Using deprecated configuration file 'workday_payslip_splitter.toml'. Please rename it to 'lm_payslip_importer.toml'.{STYLE_WARNING:#}" };
                let content = std::fs::read_to_string(&fallback_candidate).context(
                    "Failed to read workday_payslip_splitter.toml from executable directory",
                )?;
                let config = config::Config::from_toml_str(&content)
                    .context("Malformed workday_payslip_splitter.toml file")?;
                return Ok(config);
            }
        }
    }

    anyhow::bail!(
        "Configuration file 'lm_payslip_importer.toml' not found in current directory or executable directory."
    )
}
