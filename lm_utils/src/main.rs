//! The `lm-utils` busybox multiplexer.
//!
//! A single binary that hosts every Lunch Money utility tool. A tool is
//! selected one of two ways:
//!
//! 1. **argv0 (busybox) dispatch** — when the binary is invoked through a
//!    symlink whose basename starts with `lm-` followed by a tool's stable name
//!    (e.g. `lm-payslip-importer`, `lm-splitwise-sync`, `lm-venmo-balfixer`),
//!    that tool runs directly, exactly as the former standalone binaries did.
//! 2. **explicit dispatch** — `lm-utils <tool> ...` selects the tool by
//!    subcommand.
//!
//! The dispatch table is a static `clap` subcommand enum: each tool exposes a
//! unit struct implementing [`lm_common::tool::Tool`] plus its clap argument
//! group ([`Tool::Cli`]), which is embedded here as a subcommand variant. The
//! shared `--dry-run` flag is hoisted to the top level (global) and flows into
//! every tool through the [`ToolContext`].

use anstream::eprintln;
use clap::Parser;
use clap::Subcommand;

use lm_common::cli::cli_styles;
use lm_common::tool::Tool;
use lm_common::tool::ToolContext;

use lm_payslip_importer::PayslipTool;
use lm_splitwise_sync::SplitwiseTool;
use lm_venmo_balfixer::VenmoTool;

/// Multiplexer for the Lunch Money utility tools.
#[derive(Parser, Debug)]
#[command(
    name = "lm-utils",
    author,
    version,
    about,
    long_about = None,
    styles = cli_styles(),
)]
struct Cli {
    /// Preview changes without modifying Lunch Money (applies to every tool).
    #[arg(long, global = true)]
    dry_run: bool,

    #[command(subcommand)]
    tool: ToolCmd,
}

/// Static dispatch table over the available tools.
///
/// Each variant embeds a tool's clap argument group ([`Tool::Cli`], an
/// `Args`-deriving type), so the variant's own subcommand tree is that tool's.
#[derive(Subcommand, Debug)]
enum ToolCmd {
    /// Import granular payslip breakdowns into Lunch Money.
    #[command(name = "payslip-importer")]
    PayslipImporter(<PayslipTool as Tool>::Cli),

    /// Sync Splitwise transactions and balances into Lunch Money.
    #[command(name = "splitwise-sync")]
    SplitwiseSync(<SplitwiseTool as Tool>::Cli),

    /// Reconcile Venmo and bank checking accounts in Lunch Money.
    #[command(name = "venmo-balfixer")]
    VenmoBalfixer(<VenmoTool as Tool>::Cli),
}

/// The stable invocation names that trigger argv0 (busybox) dispatch.
const TOOL_NAMES: &[&str] = &[PayslipTool::NAME, SplitwiseTool::NAME, VenmoTool::NAME];

#[tokio::main(flavor = "current_thread")]
async fn main() {
    if let Err(err) = run().await {
        use lm_common::style::STYLE_ERROR;
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

    let cli = Cli::parse_from(busybox_argv());
    let cx = ToolContext::new(cli.dry_run);
    match cli.tool {
        ToolCmd::PayslipImporter(args) => PayslipTool::run(&cx, args).await,
        ToolCmd::SplitwiseSync(args) => SplitwiseTool::run(&cx, args).await,
        ToolCmd::VenmoBalfixer(args) => VenmoTool::run(&cx, args).await,
    }
}

/// Rewrites the process argv for busybox dispatch.
///
/// If the binary was invoked through a symlink whose basename is `lm-` followed
/// by a known tool name, the tool name is spliced in as the first argument so
/// the unified clap parser routes to that tool's subcommand. Otherwise argv is
/// returned unchanged for ordinary `lm-utils <tool> ...` parsing.
fn busybox_argv() -> Vec<std::ffi::OsString> {
    let mut args: Vec<std::ffi::OsString> = std::env::args_os().collect();

    let basename = args
        .first()
        .and_then(|a| std::path::Path::new(a).file_name())
        .map(|s| s.to_string_lossy().into_owned());

    if let Some(basename) = basename {
        if let Some(tool_name) = basename.strip_prefix("lm-") {
            if TOOL_NAMES.contains(&tool_name) {
                // Splice the resolved tool name in as the subcommand, and normalize
                // argv[0] to the unified binary name so clap's usage/help reads as
                // `lm-utils <tool> ...` rather than doubling the symlink basename.
                args[0] = std::ffi::OsString::from("lm-utils");
                args.insert(1, std::ffi::OsString::from(tool_name));
            }
        }
    }

    args
}
