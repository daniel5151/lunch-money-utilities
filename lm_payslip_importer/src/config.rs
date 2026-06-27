use std::collections::HashMap;

use serde::Deserialize;

use crate::payslip::PayslipKind;

/// Importer configuration.
///
/// The shared Lunch Money API key now lives in `[common].lm_api_key`
/// ([`lm_common::config::CommonConfig`]); this `[payslip]` section holds the
/// importer's own provider-independent `tag` plus everything that is
/// intrinsically provider-specific — the category mapping, the payee stamped on
/// splits, the deposit account, the imputed-income exceptions, and the
/// Workday-only RSU plumbing — in a per-provider [`BackendConfig`], keyed by
/// [`PayslipKind`] under `[payslip.backends.<kind>]`. The importer selects the
/// backend matching the PDF it detected.
///
/// ```toml
/// [common]
/// lm_api_key = "..."
///
/// [payslip]
/// tag = "payslip"
///
/// [payslip.backends.workday]
/// net_zero_account = "Checking"
/// payslip_payee = "Meta Payslip"
/// rsu_account = "Equity Awards"
/// rsu_payee_match = "$META Vest"
/// [payslip.backends.workday.mapping]
/// "Salary" = "Salary"
/// ```
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Optional tag stamped on transactions created by the importer.
    #[serde(default)]
    pub tag: Option<String>,
    pub backends: HashMap<PayslipKind, BackendConfig>,
}

/// All settings that depend on which payroll provider produced a payslip.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackendConfig {
    /// Account where zero-dollar check matches or direct-deposit splits post.
    pub net_zero_account: String,
    /// Payee stamped on newly created direct-deposit / net-zero transactions.
    pub payslip_payee: String,
    /// Manual account tracking RSU vests. Required for providers that encode
    /// RSU vests as separate $0 paychecks (see
    /// [`PayslipKind::uses_rsu_reconstruction`]); `None` otherwise.
    #[serde(default)]
    pub rsu_account: Option<String>,
    /// Payee of the auto-imported $0.00 RSU vest transaction to match against
    /// (case-insensitive). Required for RSU-reconstruction providers; `None`
    /// otherwise.
    #[serde(default)]
    pub rsu_payee_match: Option<String>,
    /// Maps payslip line descriptions to Lunch Money category names (or IDs).
    #[serde(default)]
    pub mapping: HashMap<String, String>,
    /// Imputed-income handling for this provider's line descriptions.
    #[serde(default)]
    pub imputed_income: ImputedIncomeConfig,
}

/// Per-backend imputed-income configuration. Only meaningful for providers that
/// inject offsets ([`PayslipKind::injects_imputed_offsets`], i.e. Workday): such
/// providers detect most imputed lines by a marker (Workday's leading `*`), and
/// this lists the *additional* descriptions that are imputed but carry no
/// marker.
#[derive(Debug, Deserialize, Default, Clone)]
#[serde(deny_unknown_fields)]
pub struct ImputedIncomeConfig {
    /// Extra line descriptions (exact, case-insensitive) to treat as imputed
    /// income, beyond those the backend detects by its own marker. For Workday
    /// these are the unmarked relocation gross-up companions (e.g.
    /// `Relocation Tax Ben`).
    #[serde(default)]
    pub descriptions: Vec<String>,
}

impl Config {
    /// Parse and validate a TOML config.
    #[cfg(test)]
    pub fn from_toml_str(s: &str) -> anyhow::Result<Self> {
        let config: Config =
            toml::from_str(s).map_err(|e| anyhow::anyhow!("Malformed configuration: {e}"))?;
        config.validate()?;
        Ok(config)
    }

    /// Enforce per-backend provider invariants.
    ///
    /// Run after deserializing the `[payslip]` section out of the unified
    /// document (serde alone does not invoke [`BackendConfig::validate`]).
    pub fn validate(&self) -> anyhow::Result<()> {
        for (kind, backend) in &self.backends {
            backend.validate(*kind)?;
        }
        Ok(())
    }

    /// The backend configuration for `kind`, or a helpful error if the user has
    /// not configured that provider.
    pub fn backend(&self, kind: PayslipKind) -> anyhow::Result<&BackendConfig> {
        self.backends.get(&kind).ok_or_else(|| {
            let mut configured: Vec<&str> = self.backends.keys().map(|k| k.as_str()).collect();
            configured.sort_unstable();
            let configured = if configured.is_empty() {
                "none".to_string()
            } else {
                configured.join(", ")
            };
            anyhow::anyhow!(
                "No configuration found for the detected payslip provider '{}'. \
                 Add a [backends.{}] section to your config (configured providers: {}).",
                kind,
                kind.as_str(),
                configured
            )
        })
    }
}

impl BackendConfig {
    /// Enforce provider invariants: providers that reconstruct RSU vests from
    /// separate $0 paychecks need the RSU account + vest payee to do so, so
    /// reject a config that omits them rather than failing mid-import.
    fn validate(&self, kind: PayslipKind) -> anyhow::Result<()> {
        if kind.uses_rsu_reconstruction() {
            if self.rsu_account.is_none() {
                anyhow::bail!(
                    "[backends.{}] requires 'rsu_account' (this provider reconstructs RSU vests).",
                    kind.as_str()
                );
            }
            if self.rsu_payee_match.is_none() {
                anyhow::bail!(
                    "[backends.{}] requires 'rsu_payee_match' (this provider reconstructs RSU vests).",
                    kind.as_str()
                );
            }
        }
        if !kind.injects_imputed_offsets() && !self.imputed_income.descriptions.is_empty() {
            anyhow::bail!(
                "[backends.{0}.imputed_income] sets 'descriptions', but the '{0}' provider \
                 does not inject imputed-income offsets (it prints both halves of non-cash \
                 items inline). Remove the [backends.{0}.imputed_income] section.",
                kind.as_str()
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modern_multi_backend_parses() {
        let toml = r#"
tag = "payslip"

[backends.workday]
net_zero_account = "Checking"
payslip_payee = "Meta Payslip"
rsu_account = "Equity Awards"
rsu_payee_match = "$META Vest"
[backends.workday.mapping]
"Salary" = "Salary"
[backends.workday.imputed_income]
descriptions = ["Relocation Tax Ben"]

[backends.microsoft]
net_zero_account = "MSFT Checking"
payslip_payee = "Microsoft Payslip"
[backends.microsoft.mapping]
"Regular" = "Salary"
"#;
        let cfg = Config::from_toml_str(toml).unwrap();
        assert_eq!(cfg.tag.as_deref(), Some("payslip"));
        let wd = cfg.backend(PayslipKind::Workday).unwrap();
        assert_eq!(wd.net_zero_account, "Checking");
        assert_eq!(wd.rsu_account.as_deref(), Some("Equity Awards"));
        assert_eq!(wd.rsu_payee_match.as_deref(), Some("$META Vest"));
        assert_eq!(wd.mapping.get("Salary").map(String::as_str), Some("Salary"));
        assert_eq!(wd.imputed_income.descriptions, vec!["Relocation Tax Ben"]);
        let ms = cfg.backend(PayslipKind::Microsoft).unwrap();
        assert_eq!(ms.net_zero_account, "MSFT Checking");
        assert!(ms.rsu_account.is_none());
        assert!(ms.rsu_payee_match.is_none());
    }

    #[test]
    fn backend_without_mapping_or_imputed_income_defaults_empty() {
        let toml = r#"
[backends.microsoft]
net_zero_account = "Checking"
payslip_payee = "Microsoft Payslip"
"#;
        let cfg = Config::from_toml_str(toml).unwrap();
        let ms = cfg.backend(PayslipKind::Microsoft).unwrap();
        assert!(ms.mapping.is_empty());
        assert!(ms.imputed_income.descriptions.is_empty());
    }

    #[test]
    fn workday_backend_requires_rsu_fields() {
        let toml = r#"
[backends.workday]
net_zero_account = "Checking"
payslip_payee = "Meta Payslip"
"#;
        let err = Config::from_toml_str(toml).unwrap_err().to_string();
        assert!(err.contains("rsu_account"), "unexpected error: {err}");
    }

    #[test]
    fn microsoft_backend_needs_no_rsu_fields() {
        let toml = r#"
[backends.microsoft]
net_zero_account = "Checking"
payslip_payee = "Microsoft Payslip"
"#;
        let cfg = Config::from_toml_str(toml).unwrap();
        assert!(
            cfg.backend(PayslipKind::Microsoft)
                .unwrap()
                .rsu_account
                .is_none()
        );
    }

    #[test]
    fn imputed_income_rejected_on_non_injecting_backend() {
        // Microsoft prints both halves of non-cash items inline and so does not
        // inject imputed offsets; configuring imputed descriptions for it is a
        // mistake the validator must catch.
        let toml = r#"
[backends.microsoft]
net_zero_account = "Checking"
payslip_payee = "Microsoft Payslip"

[backends.microsoft.imputed_income]
descriptions = ["Some Line"]
"#;
        let err = Config::from_toml_str(toml).unwrap_err().to_string();
        assert!(
            err.contains("does not inject imputed-income offsets"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn unknown_field_is_rejected() {
        let toml = r#"
[backends.workday]
net_zero_account = "Checking"
payslip_payee = "Meta Payslip"
rsu_account = "Equity Awards"
rsu_payee_match = "$META Vest"
bogus_field = "x"
"#;
        assert!(Config::from_toml_str(toml).is_err());
    }

    #[test]
    fn missing_backends_table_is_rejected() {
        let toml = r#"
tag = "payslip"
"#;
        assert!(Config::from_toml_str(toml).is_err());
    }
}
