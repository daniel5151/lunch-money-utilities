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
                let config = load_config(&reconcile_args.config)?;
                commands::reconcile::run_reconcile(cx, &config, reconcile_args).await?;
            }
        }
        Ok(())
    }
}

fn load_config(config_path: &std::path::Path) -> anyhow::Result<config::Config> {
    use anyhow::Context;

    let read_and_parse = |path: &std::path::Path| -> anyhow::Result<config::Config> {
        let content = std::fs::read_to_string(path).context(format!(
            "Failed to read config file from {}",
            path.display()
        ))?;
        config::Config::from_toml_str(&content)
            .context(format!("Malformed config file {}", path.display()))
    };

    // 1. Check specified / default path in current working directory
    if config_path.exists() {
        return read_and_parse(config_path);
    }

    // 2. Check directory of the running executable
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let candidate = exe_dir.join(config_path);
            if candidate.exists() {
                return read_and_parse(&candidate);
            }
        }
    }

    anyhow::bail!(
        "Configuration file '{}' not found in current directory or executable directory. Please run the init wizard to generate one: lm-utils venmo-balfixer init",
        config_path.display()
    )
}
