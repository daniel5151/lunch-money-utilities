use clap::Parser;
use clap::Subcommand;
use std::path::PathBuf;

fn cli_styles() -> clap::builder::styling::Styles {
    clap::builder::styling::Styles::styled()
        .header(
            clap::builder::styling::Style::new()
                .bold()
                .fg_color(Some(clap::builder::styling::AnsiColor::BrightBlue.into())),
        )
        .usage(
            clap::builder::styling::Style::new()
                .bold()
                .fg_color(Some(clap::builder::styling::AnsiColor::BrightBlue.into())),
        )
        .literal(
            clap::builder::styling::Style::new()
                .fg_color(Some(clap::builder::styling::AnsiColor::Cyan.into())),
        )
        .placeholder(
            clap::builder::styling::Style::new()
                .fg_color(Some(clap::builder::styling::AnsiColor::BrightBlack.into())),
        )
}

#[derive(Parser, Debug)]
#[command(
    name = "lm-venmo-balfixer",
    about = "Automatically reconcile Venmo and Bank checking accounts in Lunch Money.",
    version,
    styles = cli_styles()
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Reconcile Venmo and Bank checking accounts
    Reconcile(ReconcileArgs),
    /// Run the interactive setup wizard to configure lm_venmo_balfixer.toml
    Init(InitArgs),
}

#[derive(Parser, Debug)]
pub struct ReconcileArgs {
    /// Path to the config TOML file
    #[arg(short, long, default_value = "lm_venmo_balfixer.toml")]
    pub config: PathBuf,

    /// Dry run: display what would be done without modifying anything in Lunch Money
    #[arg(long)]
    pub dry_run: bool,

    /// Time duration from today to scan for transactions (e.g., "30d", "2w", "3mon")
    #[arg(value_name = "TIME_SPAN", value_parser = parse_duration)]
    pub duration: jiff::Span,
}

#[derive(Parser, Debug)]
pub struct InitArgs {
    /// Output file path
    #[arg(long, short)]
    pub file: Option<PathBuf>,
}

fn parse_duration(s: &str) -> Result<jiff::Span, String> {
    let duration = humantime::parse_duration(s).map_err(|e| format!("invalid duration: {}", e))?;
    let secs = duration.as_secs();
    // Round up to at least 1 day
    let days = secs.div_ceil(86400);
    Ok(jiff::Span::new().days(days as i32))
}
