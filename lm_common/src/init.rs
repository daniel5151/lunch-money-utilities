//! Shared `init` wizard prompt helpers.
//!
//! Every tool's `init` wizard previously hand-rolled the same
//! [`inquire::Password`] prompt for the Lunch Money developer API key, and both
//! venmo and (historically) splitwise wrapped [`lunch_money`]'s Plaid-account
//! type in a private `Display` newtype to drive an [`inquire::Select`] picker.
//! Those duplicates live here now so each wizard only expresses its own
//! tool-specific prompts.

use anyhow::Context;
use lunch_money::plaid_accounts::schemas::PlaidAccount;

/// Prompts for the shared Lunch Money developer API key.
///
/// A masked, single-entry [`inquire::Password`] prompt — the same one every
/// tool's `init` wizard used before the key was unified into
/// `[common].lm_api_key`.
pub fn prompt_lm_api_key() -> anyhow::Result<String> {
    inquire::Password::new("Lunch Money API Key:")
        .with_help_message("Your Lunch Money developer API key")
        .with_display_mode(inquire::PasswordDisplayMode::Masked)
        .without_confirmation()
        .prompt()
        .context("Failed to get Lunch Money API Key")
}

/// A Plaid account wrapped for display in an [`inquire::Select`] list.
///
/// Renders the account's display name, falling back to its raw name, and
/// appending the raw name in parentheses when the two differ — so the picker
/// disambiguates accounts that share a friendly name.
#[derive(Clone)]
pub struct PlaidAccountChoice(pub PlaidAccount);

impl PlaidAccountChoice {
    /// The account's configured display name, falling back to its raw name.
    pub fn config_name(&self) -> String {
        self.0
            .display_name
            .clone()
            .unwrap_or_else(|| self.0.name.clone())
    }
}

impl std::fmt::Display for PlaidAccountChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0.display_name {
            Some(disp) if disp != &self.0.name => write!(f, "{} ({})", disp, self.0.name),
            Some(disp) => write!(f, "{disp}"),
            None => write!(f, "{}", self.0.name),
        }
    }
}

/// Presents an [`inquire::Select`] picker over the given Plaid accounts.
///
/// `prompt` is the question shown to the user (e.g. "Select Venmo Plaid
/// account:"). Returns the chosen account wrapped in a [`PlaidAccountChoice`]
/// so callers can read its [`config_name`](PlaidAccountChoice::config_name).
pub fn select_plaid_account(
    prompt: &str,
    choices: Vec<PlaidAccountChoice>,
) -> anyhow::Result<PlaidAccountChoice> {
    inquire::Select::new(prompt, choices)
        .prompt()
        .with_context(|| format!("Failed to select account for prompt: {prompt}"))
}
