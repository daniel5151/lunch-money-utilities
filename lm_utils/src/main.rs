//! The `lm-utils` busybox multiplexer.
//!
//! A single binary that hosts every Lunch Money utility tool. A tool is
//! selected one of two ways:
//!
//! 1. **argv0 (busybox) dispatch** — when the binary is invoked through a
//!    symlink whose basename starts with `lm-` followed by a tool's stable name
//!    (e.g. `lm-payslip-importer`, `lm-splitwise-sync`, `lm-venmo-plaidfix`),
//!    that tool runs directly, exactly as the former standalone binaries did.
//! 2. **explicit dispatch** — `lm-utils <tool> ...` selects the tool by
//!    subcommand.
//!
//! The dispatch table is a static `clap` subcommand enum: each tool exposes a
//! unit struct implementing [`lm_common::tool::Tool`] plus its clap argument
//! group ([`Tool::Cli`]), which is embedded here as a subcommand variant. The
//! shared `--dry-run` flag is hoisted to the top level (global) and flows into
//! every tool through the [`ToolContext`].

use std::path::Path;
use std::path::PathBuf;

use anstream::eprintln;
use anyhow::Context;
use clap::Parser;
use clap::Subcommand;
use lm_common::cli::cli_styles;
use lm_common::tool::Tool;
use lm_common::tool::ToolContext;
use lm_payslip_importer::PayslipTool;
use lm_backup::BackupTool;
use lm_query::QueryTool;
use lm_splitwise_sync::SplitwiseTool;
use lm_venmo_plaidfix::VenmoTool;

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

    /// Path to the configuration file (defaults to lm_utils.toml).
    #[arg(long, global = true, short = 'c')]
    config: Option<PathBuf>,

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

    /// Back up all Lunch Money data to local JSON files.
    #[command(name = "backup")]
    Backup(<BackupTool as Tool>::Cli),

    /// Query Lunch Money data (categories, tags, accounts).
    #[command(name = "query")]
    Query(<QueryTool as Tool>::Cli),

    /// Sync Splitwise transactions and balances into Lunch Money.
    #[command(name = "splitwise-sync")]
    SplitwiseSync(<SplitwiseTool as Tool>::Cli),

    /// Reconcile Venmo and bank checking accounts in Lunch Money.
    #[command(name = "venmo-plaidfix")]
    VenmoPlaidfix(<VenmoTool as Tool>::Cli),
}

/// The stable invocation names that trigger argv0 (busybox) dispatch.
const TOOL_NAMES: &[&str] = &[BackupTool::NAME, PayslipTool::NAME, QueryTool::NAME, SplitwiseTool::NAME, VenmoTool::NAME];

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

    let is_init = match &cli.tool {
        ToolCmd::PayslipImporter(args) => {
            matches!(args.command, lm_payslip_importer::cli::Commands::Init(_))
        }
        ToolCmd::Backup(_) => false,
        ToolCmd::Query(_) => false,
        ToolCmd::SplitwiseSync(args) => {
            matches!(args.command, lm_splitwise_sync::cli::Commands::Init(_))
        }
        ToolCmd::VenmoPlaidfix(args) => {
            matches!(args.command, lm_venmo_plaidfix::cli::Commands::Init(_))
        }
    };

    let resolved_path = resolve_config_path(cli.config.as_deref());

    let (common_config, doc_opt) = if resolved_path.exists() {
        let path: &Path = &resolved_path;
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file from {}", path.display()))?;
        let doc: toml_edit::DocumentMut = content
            .parse()
            .with_context(|| format!("Malformed config file {}", path.display()))?;
        let common = lm_common::config::common_section(&doc)?;
        (common, Some(doc))
    } else {
        if !is_init {
            if cli.config.is_none() {
                anyhow::bail!(
                    "Configuration file '{}' not found in current directory or \
                     executable directory. Run the relevant tool's `init` subcommand to generate one \
                     (e.g. `lm-utils venmo-plaidfix init`).",
                    lm_common::config::DEFAULT_CONFIG_FILENAME
                );
            } else {
                anyhow::bail!(
                    "Configuration file '{}' not found.",
                    resolved_path.display()
                );
            }
        }
        (lm_common::config::CommonConfig::default(), None)
    };

    match cli.tool {
        ToolCmd::PayslipImporter(args) => {
            let tool_cfg = tool_section::<PayslipTool>(&doc_opt)?;
            PayslipTool::run(&cx, args, resolved_path, common_config, tool_cfg).await
        }
        ToolCmd::Backup(args) => {
            let tool_cfg = tool_section::<BackupTool>(&doc_opt)?;
            BackupTool::run(&cx, args, resolved_path, common_config, tool_cfg).await
        }
        ToolCmd::Query(args) => {
            let tool_cfg = tool_section::<QueryTool>(&doc_opt)?;
            QueryTool::run(&cx, args, resolved_path, common_config, tool_cfg).await
        }
        ToolCmd::SplitwiseSync(args) => {
            let tool_cfg = tool_section::<SplitwiseTool>(&doc_opt)?;
            SplitwiseTool::run(&cx, args, resolved_path, common_config, tool_cfg).await
        }
        ToolCmd::VenmoPlaidfix(args) => {
            let tool_cfg = tool_section::<VenmoTool>(&doc_opt)?;
            VenmoTool::run(&cx, args, resolved_path, common_config, tool_cfg).await
        }
    }
}

/// Extract a tool's config section from the parsed document, if present.
fn tool_section<T: Tool>(
    doc_opt: &Option<toml_edit::DocumentMut>,
) -> anyhow::Result<Option<T::Config>> {
    match doc_opt {
        Some(doc) => lm_common::config::optional_section::<T::Config>(doc, T::CONFIG_SECTION),
        None => Ok(None),
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
        .and_then(|a| Path::new(a).file_name())
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

/// Locates the `lm_utils.toml` configuration file.
///
/// Searches the user-provided path first (if any), then checks the current working
/// directory, and finally checks the directory of the running executable.
fn resolve_config_path(user_path: Option<&Path>) -> PathBuf {
    if let Some(path) = user_path {
        return path.to_path_buf();
    }

    let filename = Path::new(lm_common::config::DEFAULT_CONFIG_FILENAME);

    // 1. Current working directory.
    if filename.exists() {
        return filename.to_path_buf();
    }

    // 2. Directory of the running executable.
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let candidate = exe_dir.join(filename);
            if candidate.exists() {
                return candidate;
            }
        }
    }

    filename.to_path_buf()
}
