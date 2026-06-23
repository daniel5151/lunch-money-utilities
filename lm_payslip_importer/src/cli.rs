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
    name = "lm-payslip-importer",
    about = "Import granular payslip breakdowns into Lunch Money.",
    version,
    styles = cli_styles()
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Import granular payslip breakdowns into Lunch Money.
    Import(ImportArgs),
    /// Run the interactive setup wizard to configure lm_payslip_importer.toml
    Init(InitArgs),
}

#[derive(Parser, Debug)]
pub struct ImportArgs {
    #[arg(help = "Path to the payslip PDF file")]
    pub payslip_pdf: PathBuf,

    #[arg(
        long,
        help = "Do not execute anything in Lunch Money, just show what would be done"
    )]
    pub dry_run: bool,

    #[arg(
        long = "page",
        help = "Specific page number(s) to process. If omitted, all pages are processed. Can be passed multiple times.",
        action = clap::ArgAction::Append
    )]
    pub pages: Vec<usize>,
}

#[derive(Parser, Debug)]
pub struct InitArgs {
    /// Path(s) to payslip PDF file(s) to seed the category mapping table
    #[arg(help = "Path(s) to payslip PDF file(s) to seed the [mapping] table")]
    pub pdfs: Vec<PathBuf>,

    /// Output file path (defaults to lm_payslip_importer.toml)
    #[arg(long, short)]
    pub file: Option<PathBuf>,
}
