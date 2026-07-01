use clap::Args;

/// Back up all Lunch Money data via the API
#[derive(Args, Debug)]
pub struct Cli {
    /// Output directory (default: ./lm-backup-{timestamp})
    #[arg(short, long)]
    pub output: Option<String>,

    /// Don't download file attachments
    #[arg(long)]
    pub skip_attachments: bool,

    /// Earliest transaction date (default: 2015-01-01)
    #[arg(long, default_value = "2015-01-01")]
    pub start_date: String,

    /// Override the Lunch Money API base URL
    #[arg(long, default_value = "https://api.lunchmoney.dev/v2")]
    pub api_url: String,
}
