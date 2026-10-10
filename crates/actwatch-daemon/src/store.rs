use actwatch_core::model::{AppEvent, ResourceSample};
use anyhow::{Context, Result};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

pub struct Store {
    core: actwatch_core::Store,
    #[allow(dead_code)]
    path: PathBuf,
    pending_events: Vec<AppEvent>,
    pending_samples: Vec<ResourceSample>,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("Failed to create DB directory: {}", parent.display()))?;
            }
        }

        // 4. DB file 0600 on create + chmod existing on open
        if path.exists() {
            let perms = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(path, perms)
                .with_context(|| format!("Failed to chmod 0600 on existing DB: {}", path.display()))?;
        } else {
            std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(false)
                .mode(0o600)
                .open(path)
                .with_context(|| format!("Failed to create DB file 0600 at: {}", path.display()))?;
        }

        let core = actwatch_core::Store::open_or_create(path)
            .with_context(|| format!("Failed to open actwatch_core store at: {}", path.display()))?;

        // Re-affirm permissions on the created/opened db file
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));

        info!(path = %path.display(), "Store opened as a thin buffer around core::Store");

        Ok(Self {
            core,
            path: path.to_path_buf(),
            pending_events: Vec::new(),
            pending_samples: Vec::new(),
        })
    }

    pub fn push_event(&mut self, event: AppEvent) {
        self.pending_events.push(event);
    }

    pub fn push_sample(&mut self, sample: ResourceSample) {
        self.pending_samples.push(sample);
    }

    #[allow(dead_code)]
    pub fn pending_count(&self) -> (usize, usize) {
        (self.pending_events.len(), self.pending_samples.len())
    }

    pub fn flush(&mut self, current_day: &str) -> Result<()> {
        if self.pending_events.is_empty() && self.pending_samples.is_empty() {
            return Ok(());
        }

        let num_events = self.pending_events.len();
        let num_samples = self.pending_samples.len();

        // 1. Commit pending batch atomically via core::Store
        match self.core.record_batch(&self.pending_events, &self.pending_samples) {
            Ok(()) => {
                debug!(
                    events = num_events,
                    samples = num_samples,
                    "Committed batch into SQLite via core::Store"
                );
                self.pending_events.clear();
                self.pending_samples.clear();
            }
            Err(e) => {
                warn!(
                    error = %e,
                    "Failed to commit batch to SQLite; keeping buffered items for next flush"
                );
                return Err(e);
            }
        }

        // 2. Truthful rollup refresh for current day
        if let Err(e) = self.core.refresh_day(current_day) {
            warn!(
                error = %e,
                day = %current_day,
                "Failed to refresh daily rollup for day"
            );
        }

        Ok(())
    }

    #[allow(dead_code)]
    pub fn core(&self) -> &actwatch_core::Store {
        &self.core
    }

    #[allow(dead_code)]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_store_permissions_on_create() {
        let temp_dir = tempfile_dir("perm_create");
        let db_path = temp_dir.join("activity.db");

        let store = Store::open(&db_path).expect("open store");
        drop(store);

        let meta = std::fs::metadata(&db_path).expect("metadata");
        let mode = meta.permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "Created DB file must have 0600 permissions");

        let _ = std::fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn test_store_chmod_existing_on_open() {
        let temp_dir = tempfile_dir("perm_existing");
        let db_path = temp_dir.join("activity.db");

        // Pre-create file with 0644
        std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .mode(0o644)
            .open(&db_path)
            .expect("create dummy db");

        let meta = std::fs::metadata(&db_path).expect("metadata");
        assert_eq!(meta.permissions().mode() & 0o777, 0o644);

        // Opening with Store::open must chmod 0600
        let store = Store::open(&db_path).expect("open store");
        drop(store);

        let meta = std::fs::metadata(&db_path).expect("metadata");
        let mode = meta.permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "Existing DB file must be chmod 0600 on open");

        let _ = std::fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn test_store_buffer_flush_and_refresh() {
        let temp_dir = tempfile_dir("buffer_flush");
        let db_path = temp_dir.join("activity.db");

        let mut store = Store::open(&db_path).expect("open store");

        // Today UTC: 2026-01-10
        let ts = 1768046400; // 2026-01-10 12:00:00 UTC
        let day = "2026-01-10";

        store.push_event(AppEvent::new(ts, "code", Some("main.rs".into()), 120));
        store.push_event(AppEvent::new(ts + 120, "away", None, 300));
        store.push_sample(ResourceSample::new(ts + 10, "code", 100, 20.0, 80000, 5.0));

        assert_eq!(store.pending_count(), (2, 1));

        store.flush(day).expect("flush");

        assert_eq!(store.pending_count(), (0, 0));

        // Verify data via core store methods
        let core = store.core();
        assert_eq!(core.daily_total_secs(day).unwrap(), 420);

        let rollups = core.read_rollup(day).unwrap();
        assert_eq!(rollups.len(), 2);
        let code_row = rollups.iter().find(|r| r.app_id == "code").unwrap();
        assert_eq!(code_row.secs, 120);
        assert_eq!(code_row.peak_rss_kb, 80000);

        let away_row = rollups.iter().find(|r| r.app_id == "away").unwrap();
        assert_eq!(away_row.secs, 300);

        let _ = std::fs::remove_dir_all(temp_dir);
    }

    fn tempfile_dir(prefix: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("actwatch_test_{prefix}_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        dir
    }
}
