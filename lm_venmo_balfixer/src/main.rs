use anstream::eprintln;

mod cli;
mod commands;
mod config;
mod style;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    if let Err(err) = run().await {
        use crate::style::STYLE_ERROR;
        eprintln! {};
        eprintln! { "{STYLE_ERROR}❌ Error:{STYLE_ERROR:#} {err}" };

        let mut causes = err.chain().skip(1).peekable();
        if causes.peek().is_some() {
            eprintln! {};
            eprintln! { "Caused by:" };
            for cause in causes {
                eprintln! { "  • {cause}" };
            }
        }
        eprintln! {};
        std::process::exit(1);
    }
}

async fn run() -> anyhow::Result<()> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    use clap::Parser;

    let args = cli::Cli::parse();

    match args.command {
        cli::Commands::Init(init_args) => {
            commands::init::run_init(init_args).await?;
        }
        cli::Commands::Reconcile(reconcile_args) => {
            let config = load_config(&reconcile_args.config)?;
            commands::reconcile::run_reconcile(&config, reconcile_args).await?;
        }
    }
    Ok(())
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
        "Configuration file '{}' not found in current directory or executable directory. Please run the init wizard to generate one: cargo run -p lm-venmo-balfixer -- init",
        config_path.display()
    )
}
