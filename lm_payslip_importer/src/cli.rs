use std::path::PathBuf;

use clap::Args;
use clap::Subcommand;

#[derive(Args, Debug)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Import granular payslip breakdowns into Lunch Money.
    Import(ImportArgs),
    /// Run the interactive setup wizard to configure lm_utils.toml
    Init(InitArgs),
}

#[derive(Args, Debug)]
pub struct ImportArgs {
    #[arg(help = "Path(s) to the payslip PDF file(s) to import", num_args = 1..)]
    pub payslip_pdfs: Vec<PathBuf>,

    #[arg(
        long,
        help = "Prompt for confirmation (yes/skip/stop) before each operation, after printing what it would do. Ignored under --dry-run."
    )]
    pub interactive: bool,

    #[arg(
        long = "page",
        help = "Specific page number(s) to process. If omitted, all pages are processed. Can be passed multiple times.",
        action = clap::ArgAction::Append
    )]
    pub pages: Vec<usize>,

    #[arg(
        long = "from-page",
        help = "Start processing imports from this page number (inclusive). Conflicts with --page.",
        conflicts_with = "pages"
    )]
    pub from_page: Option<usize>,
}

#[derive(Args, Debug)]
pub struct InitArgs {
    /// Path(s) to payslip PDF file(s) to seed the category mapping table
    #[arg(help = "Path(s) to payslip PDF file(s) to seed the [mapping] table")]
    pub pdfs: Vec<PathBuf>,

    /// Skip interactive logic and just print the LLM prompt for categorizing sections
    #[arg(long)]
    pub just_categorize: bool,
}
