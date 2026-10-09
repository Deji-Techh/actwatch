use crate::models::{AppEvent, ResourceSample};
use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

pub struct Store {
    conn: Connection,
    _path: PathBuf,
    pending_events: Vec<AppEvent>,
    pending_samples: Vec<ResourceSample>,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create DB directory: {}", parent.display()))?;
        }

        let conn = Connection::open(path)
            .with_context(|| format!("Failed to open SQLite database at {}", path.display()))?;

        // Lock tolerance & WAL mode per architecture spec
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA busy_timeout = 5000;",
        )?;

        // Initialize schema per docs/ARCHITECTURE.md
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS app_events (
                ts INTEGER,
                app_id TEXT,
                title TEXT,
                dur_secs INTEGER
            );

            CREATE TABLE IF NOT EXISTS resource_samples (
                ts INTEGER,
                app_id TEXT,
                pid INTEGER,
                cpu_pct REAL,
                rss_kb INTEGER,
                gpu_pct REAL
            );

            CREATE TABLE IF NOT EXISTS daily_rollup (
                day TEXT,
                app_id TEXT,
                secs INTEGER,
                avg_cpu REAL,
                peak_rss_kb INTEGER,
                avg_gpu REAL,
                PRIMARY KEY (day, app_id)
            );

            CREATE TABLE IF NOT EXISTS meta (
                key TEXT PRIMARY KEY,
                value TEXT
            );

            CREATE INDEX IF NOT EXISTS idx_app_events_ts ON app_events(ts);
            CREATE INDEX IF NOT EXISTS idx_app_events_app_id ON app_events(app_id);
            CREATE INDEX IF NOT EXISTS idx_resource_samples_ts ON resource_samples(ts);
            CREATE INDEX IF NOT EXISTS idx_resource_samples_app_id ON resource_samples(app_id);
            CREATE UNIQUE INDEX IF NOT EXISTS idx_daily_rollup_pk ON daily_rollup(day, app_id);",
        )?;

        conn.execute(
            "INSERT OR IGNORE INTO meta (key, value) VALUES ('schema_version', '1')",
            [],
        )?;

        info!(path = %path.display(), "SQLite store opened with WAL mode");

        Ok(Self {
            conn,
            _path: path.to_path_buf(),
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

    pub fn flush(&mut self) -> Result<()> {
        if self.pending_events.is_empty() && self.pending_samples.is_empty() {
            return Ok(());
        }

        let num_events = self.pending_events.len();
        let num_samples = self.pending_samples.len();

        let tx = match self.conn.transaction() {
            Ok(tx) => tx,
            Err(e) => {
                warn!(error = %e, "Could not start transaction for SQLite batch write (locked?); retrying next tick");
                return Err(e.into());
            }
        };

        // 1. Insert app_events
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO app_events (ts, app_id, title, dur_secs) VALUES (?1, ?2, ?3, ?4)",
            )?;
            for event in &self.pending_events {
                stmt.execute(params![
                    event.ts,
                    event.app_id,
                    event.title,
                    event.dur_secs
                ])?;
            }
        }

        // 2. Insert resource_samples
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO resource_samples (ts, app_id, pid, cpu_pct, rss_kb, gpu_pct) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for sample in &self.pending_samples {
                stmt.execute(params![
                    sample.ts,
                    sample.app_id,
                    sample.pid,
                    sample.cpu_pct,
                    sample.rss_kb,
                    sample.gpu_pct
                ])?;
            }
        }

        // 3. Update daily_rollup incrementally
        {
            let mut rollup_dur_stmt = tx.prepare_cached(
                "INSERT INTO daily_rollup (day, app_id, secs, avg_cpu, peak_rss_kb, avg_gpu)
                 VALUES (?1, ?2, ?3, 0.0, 0, 0.0)
                 ON CONFLICT(day, app_id) DO UPDATE SET
                     secs = secs + excluded.secs",
            )?;

            for event in &self.pending_events {
                let day = chrono::DateTime::from_timestamp(event.ts, 0)
                    .map(|dt| dt.naive_local().date().to_string())
                    .unwrap_or_else(|| "1970-01-01".to_string());

                rollup_dur_stmt.execute(params![day, event.app_id, event.dur_secs])?;
            }

            let mut rollup_sample_stmt = tx.prepare_cached(
                "INSERT INTO daily_rollup (day, app_id, secs, avg_cpu, peak_rss_kb, avg_gpu)
                 VALUES (?1, ?2, 0, ?3, ?4, ?5)
                 ON CONFLICT(day, app_id) DO UPDATE SET
                     peak_rss_kb = MAX(peak_rss_kb, excluded.peak_rss_kb)",
            )?;

            for sample in &self.pending_samples {
                let day = chrono::DateTime::from_timestamp(sample.ts, 0)
                    .map(|dt| dt.naive_local().date().to_string())
                    .unwrap_or_else(|| "1970-01-01".to_string());

                rollup_sample_stmt.execute(params![
                    day,
                    sample.app_id,
                    sample.cpu_pct,
                    sample.rss_kb,
                    sample.gpu_pct
                ])?;
            }
        }

        match tx.commit() {
            Ok(()) => {
                debug!(
                    events = num_events,
                    samples = num_samples,
                    "Committed batch into SQLite"
                );
                self.pending_events.clear();
                self.pending_samples.clear();
                Ok(())
            }
            Err(e) => {
                warn!(
                    error = %e,
                    "Failed to commit SQLite batch; keeping buffered items for next flush"
                );
                Err(e.into())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_store_open_and_flush() {
        let temp_dir = tempfile_dir();
        let db_path = temp_dir.join("test_activity.db");

        let mut store = Store::open(&db_path).expect("open store");

        store.push_event(AppEvent {
            ts: 1700000000,
            app_id: "test-app".to_string(),
            title: "Test App".to_string(),
            dur_secs: 42,
        });

        store.push_sample(ResourceSample {
            ts: 1700000005,
            app_id: "test-app".to_string(),
            pid: 1234,
            cpu_pct: 12.5,
            rss_kb: 45000,
            gpu_pct: 3.2,
        });

        store.flush().expect("flush store");

        // Verify data in DB
        let (event_count, sample_count): (i64, i64) = store
            .conn
            .query_row(
                "SELECT (SELECT COUNT(*) FROM app_events), (SELECT COUNT(*) FROM resource_samples)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("query counts");

        assert_eq!(event_count, 1);
        assert_eq!(sample_count, 1);

        // Verify daily_rollup
        let (secs, peak_rss): (i64, i64) = store
            .conn
            .query_row(
                "SELECT secs, peak_rss_kb FROM daily_rollup WHERE app_id = 'test-app'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("query rollup");

        assert_eq!(secs, 42);
        assert_eq!(peak_rss, 45000);

        let _ = std::fs::remove_dir_all(temp_dir);
    }

    fn tempfile_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("actwatch_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        dir
    }
}
