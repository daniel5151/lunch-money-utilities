mod backup;
pub mod cli;
mod raw_client;

pub use cli::Cli;
use lm_common::tool::Tool;
use lm_common::tool::ToolContext;

/// The Lunch Money backup tool.
pub struct BackupTool;

#[derive(Debug, serde::Deserialize)]
pub struct Config {}

impl Tool for BackupTool {
    const NAME: &'static str = "backup";
    const CONFIG_SECTION: &'static str = "backup";
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

        let retry = match common_config.retry {
            lm_common::config::RetryConfig::Fail => (0, std::time::Duration::from_secs(2)),
            lm_common::config::RetryConfig::Retry(fields) => {
                (fields.max_attempts, fields.initial_delay)
            }
        };

        let client = raw_client::RawClient::new(
            cx.http.clone(),
            lm_api_key,
            cli.api_url,
            retry.0,
            retry.1,
        );

        let output_dir = match cli.output {
            Some(dir) => std::path::PathBuf::from(dir),
            None => {
                let now = jiff::Zoned::now();
                let stamp = now.strftime("%Y-%m-%dT%H-%M-%S").to_string();
                std::path::PathBuf::from(format!("lm-backup-{}", stamp))
            }
        };

        backup::run(&client, &output_dir, &cli.start_date, cli.skip_attachments).await
    }
}
