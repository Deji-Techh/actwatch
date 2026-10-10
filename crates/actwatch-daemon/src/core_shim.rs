//! Isolated shim module for missing v2 contract APIs from `actwatch-core`.
//!
//! When `actwatch-core` implements and exports these APIs directly, this shim
//! can be removed and replaced with direct imports from `actwatch_core`.

pub mod model {
    /// One focused-window span (`ts` = unix seconds).
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct AppEvent {
        pub ts: i64,
        pub app_id: String,
        /// Window title after [`crate::core_shim::TitlePolicy`] redaction (`None` = redacted).
        pub title: Option<String>,
        pub dur_secs: i64,
    }

    impl AppEvent {
        #[allow(dead_code)]
        pub fn new(
            ts: i64,
            app_id: impl Into<String>,
            title: Option<String>,
            dur_secs: i64,
        ) -> Self {
            Self {
                ts,
                app_id: app_id.into(),
                title,
                dur_secs,
            }
        }
    }

    /// One per-PID resource sample (`ts` = unix seconds).
    #[derive(Debug, Clone, PartialEq)]
    pub struct ResourceSample {
        pub ts: i64,
        pub app_id: String,
        pub pid: i64,
        pub cpu_pct: f64,
        pub rss_kb: i64,
        pub gpu_pct: f64,
    }

    impl ResourceSample {
        #[allow(dead_code)]
        pub fn new(
            ts: i64,
            app_id: impl Into<String>,
            pid: i64,
            cpu_pct: f64,
            rss_kb: i64,
            gpu_pct: f64,
        ) -> Self {
            Self {
                ts,
                app_id: app_id.into(),
                pid,
                cpu_pct,
                rss_kb,
                gpu_pct,
            }
        }
    }
}

pub mod titles {
    use anyhow::{bail, Result};
    use std::path::PathBuf;

    /// Window-title privacy policy (default [`TitlePolicy::Off`]).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub enum TitlePolicy {
        /// Drop titles (`redact` -> `None`, stored as `''`).
        #[default]
        Off,
        /// Store a stable std-only FNV-1a 64-bit hex digest.
        Hash,
        /// Store the full title verbatim.
        Full,
    }

    impl TitlePolicy {
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

        pub fn from_toml_str(s: &str) -> Result<Self> {
            #[derive(serde::Deserialize, Default)]
            struct ConfigToml {
                #[serde(default)]
                privacy: PrivacyTable,
            }
            #[derive(serde::Deserialize, Default)]
            struct PrivacyTable {
                #[serde(default)]
                titles: Option<String>,
            }
            let parsed: ConfigToml =
                toml::from_str(s).map_err(|e| anyhow::anyhow!("parse privacy TOML: {e}"))?;
            match parsed.privacy.titles {
                Some(v) => Self::parse(&v),
                None => Ok(TitlePolicy::Off),
            }
        }

        pub fn load() -> Self {
            let path = default_categories_path();
            if let Ok(content) = std::fs::read_to_string(&path) {
                Self::from_toml_str(&content).unwrap_or(TitlePolicy::Off)
            } else {
                TitlePolicy::Off
            }
        }

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

    fn default_categories_path() -> PathBuf {
        if let Ok(config_home) = std::env::var("XDG_CONFIG_HOME") {
            if !config_home.trim().is_empty() {
                return PathBuf::from(config_home).join("actwatch").join("categories.toml");
            }
        }
        if let Ok(home) = std::env::var("HOME") {
            if !home.trim().is_empty() {
                return PathBuf::from(home)
                    .join(".config")
                    .join("actwatch")
                    .join("categories.toml");
            }
        }
        PathBuf::from("categories.toml")
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
}

pub use titles::TitlePolicy;

/// Extension trait providing the v2 contract APIs `record_batch` and `refresh_day`
/// on `actwatch_core::Store`.
pub trait StoreV2Ext {
    fn record_batch(
        &mut self,
        events: &[model::AppEvent],
        samples: &[model::ResourceSample],
    ) -> anyhow::Result<()>;

    fn refresh_day(&self, day: &str) -> anyhow::Result<()>;
}

impl StoreV2Ext for actwatch_core::Store {
    fn record_batch(
        &mut self,
        events: &[model::AppEvent],
        samples: &[model::ResourceSample],
    ) -> anyhow::Result<()> {
        self.batch(|tx| {
            for ev in events {
                let title = ev.title.as_deref().unwrap_or("");
                actwatch_core::Store::insert_event(tx, ev.ts, &ev.app_id, title, ev.dur_secs)?;
            }
            for s in samples {
                actwatch_core::Store::insert_resource(
                    tx,
                    s.ts,
                    &s.app_id,
                    s.pid,
                    s.cpu_pct,
                    s.rss_kb,
                    s.gpu_pct,
                )?;
            }
            Ok(())
        })
    }

    fn refresh_day(&self, day: &str) -> anyhow::Result<()> {
        self.refresh_daily_rollup(day)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_policy_default_is_off() {
        assert_eq!(TitlePolicy::default(), TitlePolicy::Off);
        assert_eq!(TitlePolicy::Off.redact("Window Title"), None);
    }

    #[test]
    fn title_policy_full() {
        assert_eq!(
            TitlePolicy::Full.redact("Window Title"),
            Some("Window Title".to_string())
        );
    }

    #[test]
    fn title_policy_hash() {
        let h1 = TitlePolicy::Hash.redact("Secret").unwrap();
        let h2 = TitlePolicy::Hash.redact("Secret").unwrap();
        let h3 = TitlePolicy::Hash.redact("Other").unwrap();
        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
        assert_eq!(h1.len(), 16);
    }

    #[test]
    fn title_policy_from_toml() {
        assert_eq!(
            TitlePolicy::from_toml_str("[privacy]\ntitles = 'hash'").unwrap(),
            TitlePolicy::Hash
        );
        assert_eq!(
            TitlePolicy::from_toml_str("[privacy]\ntitles = 'full'").unwrap(),
            TitlePolicy::Full
        );
        assert_eq!(
            TitlePolicy::from_toml_str("[privacy]\ntitles = 'off'").unwrap(),
            TitlePolicy::Off
        );
        assert_eq!(
            TitlePolicy::from_toml_str("").unwrap(),
            TitlePolicy::Off
        );
    }

    #[test]
    fn store_v2_ext_roundtrip() {
        let mut core = actwatch_core::Store::open_in_memory().unwrap();
        let events = [
            model::AppEvent::new(1700000000, "firefox", Some("Web".into()), 120),
            model::AppEvent::new(1700000120, "away", None, 300),
        ];
        let samples = [
            model::ResourceSample::new(1700000005, "firefox", 1234, 15.0, 50000, 2.5),
        ];

        core.record_batch(&events, &samples).unwrap();
        assert_eq!(core.daily_total_secs("2023-11-14").unwrap(), 420);
        core.refresh_day("2023-11-14").unwrap();

        let rollups = core.read_rollup("2023-11-14").unwrap();
        assert!(rollups.iter().any(|r| r.app_id == "firefox" && r.secs == 120));
        assert!(rollups.iter().any(|r| r.app_id == "away" && r.secs == 300));
    }
}
