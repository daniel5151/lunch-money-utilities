//! Terminal/process bootstrap helpers shared by every tool binary:
//! the TLS provider install and the pretty error-chain printer.

use anstream::eprintln;

/// Installs the process-wide rustls crypto provider (`ring`).
///
/// Must be called once, before any TLS connection is established. Panics if a
/// provider has already been installed, matching the previous per-tool behavior.
pub fn install_crypto_provider() {
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");
}

/// Prints an `anyhow` error and its cause chain to stderr in the shared format,
/// then exits the process with status 1.
///
/// This is the single implementation of the `❌ Error` + `Caused by:` walk that
/// each tool binary previously duplicated in its `main`.
pub fn report_error_and_exit(err: &anyhow::Error) -> ! {
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
