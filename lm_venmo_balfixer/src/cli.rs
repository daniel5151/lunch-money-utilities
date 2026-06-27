use clap::Args;
use clap::Subcommand;

#[derive(Args, Debug)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Reconcile Venmo and Bank checking accounts
    Reconcile(ReconcileArgs),
    /// Run the interactive setup wizard to configure lm_utils.toml
    Init(InitArgs),
}

#[derive(Args, Debug)]
pub struct ReconcileArgs {
    /// Time duration from today to scan for transactions (e.g., "30d", "2w", "3mon")
    #[arg(value_name = "TIME_SPAN", value_parser = parse_duration)]
    pub duration: jiff::Span,
}

#[derive(Args, Debug)]
pub struct InitArgs {}

fn parse_duration(s: &str) -> Result<jiff::Span, String> {
    let duration = humantime::parse_duration(s).map_err(|e| format!("invalid duration: {}", e))?;
    let secs = duration.as_secs();
    // Round up to at least 1 day
    let days = secs.div_ceil(86400);
    Ok(jiff::Span::new().days(days as i32))
}
