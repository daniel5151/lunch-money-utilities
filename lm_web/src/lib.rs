pub mod runner;
pub mod schema;
pub mod server;

use std::path::PathBuf;

use clap::Args;
use lm_common::tool::Tool;
use lm_common::tool::ToolContext;
pub use schema::CommandSchema;
pub use schema::introspect_command;
pub use server::run_server;

/// CLI arguments for the `lm-utils web` tool.
#[derive(Args, Debug, Clone)]
pub struct WebCli {
    /// Port to listen on (default 3000).
    #[arg(long, default_value = "3000")]
    pub port: u16,

    /// Host to bind to (default 127.0.0.1).
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,

    /// Do not automatically open the web browser on startup.
    #[arg(long)]
    pub no_open: bool,
}

/// The embedded Web GUI server tool for Lunch Money Utilities.
pub struct WebTool;

impl WebTool {
    pub async fn run_with_schema(
        _cx: &ToolContext,
        cli: WebCli,
        config_path: PathBuf,
        schema: CommandSchema,
    ) -> anyhow::Result<()> {
        run_server(cli.host, cli.port, !cli.no_open, schema, config_path).await
    }
}

impl Tool for WebTool {
    const NAME: &'static str = "web";
    const CONFIG_SECTION: &'static str = "web";
    type Cli = WebCli;
    type Config = serde_json::Value;

    async fn run(
        cx: &ToolContext,
        cli: WebCli,
        config_path: PathBuf,
        _common_config: lm_common::config::CommonConfig,
        _tool_config: Option<Self::Config>,
    ) -> anyhow::Result<()> {
        let cmd = WebCli::augment_args(clap::Command::new(Self::NAME));
        let schema = introspect_command(&cmd);
        Self::run_with_schema(cx, cli, config_path, schema).await
    }
}
