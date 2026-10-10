//! Window-title privacy: `core::TitlePolicy::{Off, Hash, Full}`.
//!
//! From `[privacy] titles = "off"|"hash"|"full"` in `categories.toml`
//! (default `Off`).
//!
//! - `Off` — [`TitlePolicy::redact`] returns `None`; the store writes `''`.
//! - `Hash` — returns a std-only FNV-1a 64-bit hex digest (16 lowercase hex
//!   chars). Stable within/across runs, not cryptographic.
//! - `Full` — returns `Some(title)` verbatim.
//!
//! UI is app-level only and never renders titles.

use anyhow::{bail, Result};

/// Window-title privacy policy (default [`TitlePolicy::Off`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TitlePolicy {
    /// Drop titles (`redact` → `None`).
    #[default]
    Off,
    /// Store a stable std-only hash hex string.
    Hash,
    /// Store the full title verbatim.
    Full,
}

impl TitlePolicy {
    /// Parse `"off"|"hash"|"full"` (case-insensitive, trimmed).
    pub fn parse(s: &str) -> Result<Self> {
        match s.trim().to_lowercase().as_str() {
            "off" => Ok(TitlePolicy::Off),
            "hash" => Ok(TitlePolicy::Hash),
            "full" => Ok(TitlePolicy::Full),
            _ => bail!("unknown title policy: {s:?} (expected off|hash|full)"),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            TitlePolicy::Off => "off",
            TitlePolicy::Hash => "hash",
            TitlePolicy::Full => "full",
        }
    }

    /// Read `[privacy] titles = ...` from a TOML string. Missing `[privacy]`
    /// or missing `titles` key → `Off` (the default).
    pub fn from_toml_str(s: &str) -> Result<Self> {
        #[derive(serde::Deserialize, Default)]
        struct PrivacyToml {
            #[serde(default)]
            privacy: PrivacyTable,
        }
        #[derive(serde::Deserialize, Default)]
        struct PrivacyTable {
            #[serde(default)]
            titles: Option<String>,
        }
        let parsed: PrivacyToml =
            toml::from_str(s).map_err(|e| anyhow::anyhow!("parse privacy TOML: {e}"))?;
        match parsed.privacy.titles {
            Some(v) => Self::parse(&v),
            None => Ok(TitlePolicy::Off),
        }
    }

    /// Apply the policy to a raw window title.
    pub fn redact(self, title: &str) -> Option<String> {
        match self {
            TitlePolicy::Off => None,
            TitlePolicy::Hash => Some(fnv1a_hex(title)),
            TitlePolicy::Full => Some(title.to_string()),
        }
    }
}

impl std::fmt::Display for TitlePolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// FNV-1a 64-bit hash rendered as 16 lowercase hex chars (std-only).
fn fnv1a_hex(s: &str) -> String {
    const OFFSET: u64 = 0xcbf29ce484222325;
    const PRIME: u64 = 0x100000001b3;
    let mut h = OFFSET;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(PRIME);
    }
    format!("{h:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_off() {
        assert_eq!(TitlePolicy::default(), TitlePolicy::Off);
    }

    #[test]
    fn parse_accepts_all_modes_case_insensitive() {
        assert_eq!(TitlePolicy::parse("off").unwrap(), TitlePolicy::Off);
        assert_eq!(TitlePolicy::parse("HASH").unwrap(), TitlePolicy::Hash);
        assert_eq!(TitlePolicy::parse(" Full ").unwrap(), TitlePolicy::Full);
        assert!(TitlePolicy::parse("redact").is_err());
    }

    #[test]
    fn off_redacts_to_none() {
        assert_eq!(TitlePolicy::Off.redact("Secret doc"), None);
    }

    #[test]
    fn full_keeps_title_verbatim() {
        assert_eq!(
            TitlePolicy::Full.redact("Secret doc"),
            Some("Secret doc".to_string())
        );
    }

    #[test]
    fn hash_is_stable_hex_and_input_sensitive() {
        let a = TitlePolicy::Hash.redact("Secret doc").unwrap();
        let b = TitlePolicy::Hash.redact("Secret doc").unwrap();
        let c = TitlePolicy::Hash.redact("secret doc").unwrap();
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(a.len(), 16);
        assert!(a.chars().all(|ch| ch.is_ascii_hexdigit()));
    }

    #[test]
    fn from_toml_str_reads_privacy_table() {
        assert_eq!(
            TitlePolicy::from_toml_str("[privacy]\ntitles = \"full\"\n").unwrap(),
            TitlePolicy::Full
        );
        assert_eq!(
            TitlePolicy::from_toml_str("[privacy]\ntitles = \"hash\"\n").unwrap(),
            TitlePolicy::Hash
        );
        // Missing table/key → Off default; unrelated keys ignored.
        assert_eq!(TitlePolicy::from_toml_str("").unwrap(), TitlePolicy::Off);
        assert_eq!(
            TitlePolicy::from_toml_str(
                "[aliases]\nfoo = \"Bar\"\n[categories]\nfoo = \"Social\"\n"
            )
            .unwrap(),
            TitlePolicy::Off
        );
        assert!(TitlePolicy::from_toml_str(
            "[privacy]\ntitles = \"sometimes\"\n"
        )
        .is_err());
    }
}
