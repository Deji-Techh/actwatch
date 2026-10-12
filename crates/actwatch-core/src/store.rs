//! SQLite store: schema v1, batched writes, aggregation queries.
//!
//! Retention policy: **forever**. This module deliberately contains no
//! `DELETE`/`DROP`/prune logic; [`Store::vacuum`] only rebuilds the file
//! (reclaims free pages) and never removes rows.

use anyhow::{Context, Result};
use chrono::NaiveDate;
use rusqlite::{params, Connection};
use std::path::Path;

use crate::categories::{Categories, Category};
use crate::model::{AppEvent, ResourceSample};

/// Current schema version, stored in `meta(schema_version)`.
pub const SCHEMA_VERSION: &str = "1";

/// Schema v1. Idempotent (`IF NOT EXISTS` everywhere).
pub const SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS app_events (
    ts        INTEGER NOT NULL,
    app_id    TEXT    NOT NULL,
    title     TEXT    NOT NULL DEFAULT '',
    dur_secs  INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_app_events_ts ON app_events (ts);
CREATE INDEX IF NOT EXISTS idx_app_events_app_ts ON app_events (app_id, ts);

CREATE TABLE IF NOT EXISTS resource_samples (
    ts        INTEGER NOT NULL,
    app_id    TEXT    NOT NULL,
    pid       INTEGER NOT NULL,
    cpu_pct   REAL    NOT NULL,
    rss_kb    INTEGER NOT NULL,
    gpu_pct   REAL    NOT NULL DEFAULT 0.0
);
CREATE INDEX IF NOT EXISTS idx_resource_samples_ts ON resource_samples (ts);
CREATE INDEX IF NOT EXISTS idx_resource_samples_app_ts ON resource_samples (app_id, ts);

CREATE TABLE IF NOT EXISTS daily_rollup (
    day          TEXT    NOT NULL,
    app_id       TEXT    NOT NULL,
    secs         INTEGER NOT NULL,
    avg_cpu      REAL    NOT NULL DEFAULT 0.0,
    peak_rss_kb  INTEGER NOT NULL DEFAULT 0,
    avg_gpu      REAL    NOT NULL DEFAULT 0.0,
    PRIMARY KEY (day, app_id)
);

CREATE TABLE IF NOT EXISTS meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
"#;

/// Per-app usage + resource aggregates over a time range.
#[derive(Debug, Clone, PartialEq)]
pub struct AppStats {
    pub app_id: String,
    pub secs: i64,
    pub avg_cpu: f64,
    pub peak_rss_kb: i64,
    pub avg_gpu: f64,
}

/// One day of the 7-day stacked series, split by category.
#[derive(Debug, Clone, PartialEq)]
pub struct DayStack {
    pub day: String,
    pub social_secs: i64,
    pub productivity_secs: i64,
    pub utilities_secs: i64,
    pub gaming_secs: i64,
    pub media_secs: i64,
    pub other_secs: i64,
    pub total_secs: i64,
}

impl DayStack {
    pub fn for_category(&self, cat: Category) -> i64 {
        match cat {
            Category::Social => self.social_secs,
            Category::ProductivityFinance => self.productivity_secs,
            Category::Utilities => self.utilities_secs,
            Category::Gaming => self.gaming_secs,
            Category::Media => self.media_secs,
            Category::Other => self.other_secs,
        }
    }
}

/// Week-over-week comparison: 7 days ending on a date vs the prior 7 days.
#[derive(Debug, Clone, PartialEq)]
pub struct WeekDelta {
    pub current_secs: i64,
    pub previous_secs: i64,
    /// `(current - previous) / previous * 100`.
    /// `None` when the previous week is empty but the current one is not
    /// (unbounded growth); `Some(0.0)` when both are empty.
    pub delta_pct: Option<f64>,
}

/// One cached `daily_rollup` row.
#[derive(Debug, Clone, PartialEq)]
pub struct RollupRow {
    pub day: String,
    pub app_id: String,
    pub secs: i64,
    pub avg_cpu: f64,
    pub peak_rss_kb: i64,
    pub avg_gpu: f64,
}

/// Unix-timestamp bounds `[start, end)` for a `YYYY-MM-DD` day (local time).
pub fn day_range(day: &str) -> Result<(i64, i64)> {
    use chrono::TimeZone;
    let date =
        NaiveDate::parse_from_str(day, "%Y-%m-%d").with_context(|| {
            format!("invalid day {day:?}, expected YYYY-MM-DD")
        })?;
    // Local midnight, NOT UTC midnight: interpreting a local day in UTC
    // silently drops the first UTC-offset hours of every day (e.g. 00:00–01:00
    // WAT belongs to the previous UTC day). DST gaps fold back one hour.
    let naive_midnight = date.and_hms_opt(0, 0, 0).context("invalid midnight")?;
    let start = chrono::Local
        .from_local_datetime(&naive_midnight)
        .single()
        .or_else(|| {
            chrono::Local
                .from_local_datetime(&naive_midnight)
                .earliest()
        })
        .context("day has no local midnight")?
        .timestamp();
    Ok((start, start + 86_400))
}

/// SQLite store. `Connection` already serializes access internally, so
/// single-row writes take `&self`; only [`Store::batch`] (a real
/// `BEGIN`/`COMMIT` transaction) needs `&mut self`.
pub struct Store {
    conn: Connection,
}

impl Store {
    fn init(conn: Connection) -> Result<Self> {
        let store = Self { conn };
        store.conn.pragma_update(None, "journal_mode", "WAL")?;
        store.conn.pragma_update(None, "synchronous", "NORMAL")?;
        store.conn.pragma_update(None, "busy_timeout", 5000)?;
        store.migrate()?;
        Ok(store)
    }

    /// Open (creating parent dirs) or create the DB file at `path`.
    pub fn open_or_create(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).with_context(|| {
                    format!("create db parent dir {}", parent.display())
                })?;
            }
        }
        let conn = Connection::open(path)
            .with_context(|| format!("open db {}", path.display()))?;
        Self::init(conn)
    }

    /// In-memory DB (tests / ephemeral use).
    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    /// Create schema objects if missing and stamp the version. Idempotent.
    pub fn migrate(&self) -> Result<()> {
        self.conn.execute_batch(SCHEMA_SQL)?;
        self.conn.execute(
            "INSERT OR IGNORE INTO meta (key, value) VALUES ('schema_version', ?1)",
            params![SCHEMA_VERSION],
        )?;
        Ok(())
    }

    pub fn schema_version(&self) -> Result<String> {
        self.conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'schema_version'",
                [],
                |row| row.get(0),
            )
            .context("read schema_version")
    }

    // ---- Write API ----

    /// Record one active-window event (`ts` = unix seconds).
    pub fn record_event(
        &self,
        ts: i64,
        app_id: &str,
        title: &str,
        dur_secs: i64,
    ) -> Result<()> {
        Self::insert_event(&self.conn, ts, app_id, title, dur_secs)
    }

    /// Record one per-PID resource sample.
    #[allow(clippy::too_many_arguments)]
    pub fn record_resource(
        &self,
        ts: i64,
        app_id: &str,
        pid: i64,
        cpu_pct: f64,
        rss_kb: i64,
        gpu_pct: f64,
    ) -> Result<()> {
        Self::insert_resource(
            &self.conn,
            ts,
            app_id,
            pid,
            cpu_pct,
            rss_kb,
            gpu_pct,
        )
    }

    /// Plural alias of [`Store::record_resource`] (spec name).
    #[allow(clippy::too_many_arguments)]
    pub fn record_resources(
        &self,
        ts: i64,
        app_id: &str,
        pid: i64,
        cpu_pct: f64,
        rss_kb: i64,
        gpu_pct: f64,
    ) -> Result<()> {
        self.record_resource(ts, app_id, pid, cpu_pct, rss_kb, gpu_pct)
    }

    pub fn insert_event(
        conn: &Connection,
        ts: i64,
        app_id: &str,
        title: &str,
        dur_secs: i64,
    ) -> Result<()> {
        conn.execute(
            "INSERT INTO app_events (ts, app_id, title, dur_secs) VALUES (?1, ?2, ?3, ?4)",
            params![ts, app_id, title, dur_secs],
        )?;
        Ok(())
    }

    pub fn insert_resource(
        conn: &Connection,
        ts: i64,
        app_id: &str,
        pid: i64,
        cpu_pct: f64,
        rss_kb: i64,
        gpu_pct: f64,
    ) -> Result<()> {
        conn.execute(
            "INSERT INTO resource_samples (ts, app_id, pid, cpu_pct, rss_kb, gpu_pct)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![ts, app_id, pid, cpu_pct, rss_kb, gpu_pct],
        )?;
        Ok(())
    }

    /// Run `f` inside a single transaction (one commit). The daemon batches
    /// ~60s of events/samples per call; tests use it for fixture setup.
    pub fn batch<T>(
        &mut self,
        f: impl FnOnce(&rusqlite::Transaction<'_>) -> Result<T>,
    ) -> Result<T> {
        let tx = self
            .conn
            .transaction()
            .context("begin batch transaction")?;
        let out = f(&tx)?;
        tx.commit().context("commit batch transaction")?;
        Ok(out)
    }

    /// Record a batch of events + samples in a single transaction (one
    /// commit). Lock-tolerant: `busy_timeout` is 5000ms (see
    /// [`Store::open_or_create`]); a still-held lock returns `Err` and the
    /// caller retains its buffers (both are borrowed slices).
    ///
    /// `AppEvent.title == None` (redacted) is stored as `''`; the schema
    /// columns are unchanged by the v2 contract.
    pub fn record_batch(
        &mut self,
        events: &[AppEvent],
        samples: &[ResourceSample],
    ) -> Result<()> {
        let tx =
            self.conn.transaction().context("begin record_batch transaction")?;
        for e in events {
            tx.execute(
                "INSERT INTO app_events (ts, app_id, title, dur_secs)
                 VALUES (?1, ?2, ?3, ?4)",
                params![e.ts, e.app_id, e.title.as_deref().unwrap_or(""), e.dur_secs],
            )?;
        }
        for s in samples {
            tx.execute(
                "INSERT INTO resource_samples (ts, app_id, pid, cpu_pct, rss_kb, gpu_pct)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![s.ts, s.app_id, s.pid, s.cpu_pct, s.rss_kb, s.gpu_pct],
            )?;
        }
        tx.commit().context("commit record_batch transaction")?;
        Ok(())
    }

    // ---- Query API ----

    /// Total focused seconds in `[start_ts, end_ts)`.
    pub fn total_secs_range(&self, start_ts: i64, end_ts: i64) -> Result<i64> {
        self.conn
            .query_row(
                "SELECT COALESCE(SUM(dur_secs), 0) FROM app_events
                 WHERE ts >= ?1 AND ts < ?2",
                params![start_ts, end_ts],
                |row| row.get(0),
            )
            .context("total_secs_range")
    }

    /// Total focused seconds on a `YYYY-MM-DD` day (UTC).
    pub fn daily_total_secs(&self, day: &str) -> Result<i64> {
        let (start, end) = day_range(day)?;
        self.total_secs_range(start, end)
    }

    /// Per-app totals over `[start_ts, end_ts)`: summed focus time plus
    /// average CPU %, peak RSS and average GPU % from resource samples.
    /// Ordered by `secs` descending.
    pub fn per_app_totals(
        &self,
        start_ts: i64,
        end_ts: i64,
    ) -> Result<Vec<AppStats>> {
        let mut stmt = self.conn.prepare(
            "SELECT app_id, COALESCE(SUM(dur_secs), 0) FROM app_events
             WHERE ts >= ?1 AND ts < ?2 GROUP BY app_id ORDER BY 2 DESC",
        )?;
        let apps: Vec<(String, i64)> = stmt
            .query_map(params![start_ts, end_ts], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })?
            .collect::<std::result::Result<_, _>>()?;
        drop(stmt);

        let mut out = Vec::with_capacity(apps.len());
        for (app_id, secs) in apps {
            let (avg_cpu, peak_rss_kb, avg_gpu): (f64, i64, f64) =
                self.conn.query_row(
                    "SELECT COALESCE(AVG(cpu_pct), 0.0), COALESCE(MAX(rss_kb), 0),
                            COALESCE(AVG(gpu_pct), 0.0)
                     FROM resource_samples
                     WHERE app_id = ?1 AND ts >= ?2 AND ts < ?3",
                    params![app_id, start_ts, end_ts],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )?;
            out.push(AppStats {
                app_id,
                secs,
                avg_cpu,
                peak_rss_kb,
                avg_gpu,
            });
        }
        Ok(out)
    }

    /// 7-day stacked series ending on `end_day` (inclusive, chronological),
    /// with each day's seconds split by category.
    pub fn seven_day_stacked(
        &self,
        end_day: &str,
        cats: &Categories,
    ) -> Result<Vec<DayStack>> {
        let end =
            NaiveDate::parse_from_str(end_day, "%Y-%m-%d").with_context(|| {
                format!("invalid end_day {end_day:?}, expected YYYY-MM-DD")
            })?;
        let mut out = Vec::with_capacity(7);
        for i in 0..7 {
            let date =
                end - chrono::Duration::days(6 - i);
            let day = date.format("%Y-%m-%d").to_string();
            let (start, stop) = day_range(&day)?;
            let mut stmt = self.conn.prepare(
                "SELECT app_id, COALESCE(SUM(dur_secs), 0) FROM app_events
                 WHERE ts >= ?1 AND ts < ?2 GROUP BY app_id",
            )?;
            let rows: Vec<(String, i64)> = stmt
                .query_map(params![start, stop], |row| {
                    Ok((row.get(0)?, row.get(1)?))
                })?
                .collect::<std::result::Result<_, _>>()?;
            drop(stmt);

            let mut stack = DayStack {
                day,
                social_secs: 0,
                productivity_secs: 0,
                utilities_secs: 0,
                gaming_secs: 0,
                media_secs: 0,
                other_secs: 0,
                total_secs: 0,
            };
            for (app_id, secs) in rows {
                match cats.categorize(&app_id) {
                    Category::Social => stack.social_secs += secs,
                    Category::ProductivityFinance => {
                        stack.productivity_secs += secs
                    }
                    Category::Utilities => stack.utilities_secs += secs,
                    Category::Gaming => stack.gaming_secs += secs,
                    Category::Media => stack.media_secs += secs,
                    Category::Other => stack.other_secs += secs,
                }
                stack.total_secs += secs;
            }
            out.push(stack);
        }
        Ok(out)
    }

    /// Compare the 7 days ending on `end_day` against the prior 7 days.
    pub fn week_over_week(&self, end_day: &str) -> Result<WeekDelta> {
        let end =
            NaiveDate::parse_from_str(end_day, "%Y-%m-%d").with_context(|| {
                format!("invalid end_day {end_day:?}, expected YYYY-MM-DD")
            })?;
        let cur_end = end
            .and_hms_opt(0, 0, 0)
            .context("invalid midnight")?
            .and_utc()
            .timestamp()
            + 86_400;
        let cur_start = cur_end - 7 * 86_400;
        let prev_start = cur_start - 7 * 86_400;
        let current_secs = self.total_secs_range(cur_start, cur_end)?;
        let previous_secs = self.total_secs_range(prev_start, cur_start)?;
        let delta_pct = if previous_secs == 0 {
            if current_secs == 0 {
                Some(0.0)
            } else {
                None
            }
        } else {
            Some(
                (current_secs - previous_secs) as f64 / previous_secs as f64
                    * 100.0,
            )
        };
        Ok(WeekDelta {
            current_secs,
            previous_secs,
            delta_pct,
        })
    }

    // ---- Rollup cache ----

    /// Rebuild that day's `daily_rollup` rows from the raw tables:
    /// `secs = SUM(dur_secs)`, `avg_cpu = AVG(cpu_pct)`,
    /// `peak_rss_kb = MAX(rss_kb)`, `avg_gpu = AVG(gpu_pct)`.
    /// The daemon calls this for the current day on every 60s commit;
    /// frontends read `daily_rollup` only.
    ///
    /// Spec name for [`Store::refresh_daily_rollup`] (same behavior).
    pub fn refresh_day(&self, day: &str) -> Result<()> {
        self.refresh_daily_rollup(day)
    }

    /// Rebuild the `daily_rollup` rows for `day` from raw events + samples.
    pub fn refresh_daily_rollup(&self, day: &str) -> Result<()> {
        let (start, end) = day_range(day)?;
        let apps = self.per_app_totals(start, end)?;
        for app in &apps {
            self.conn.execute(
                "INSERT OR REPLACE INTO daily_rollup
                 (day, app_id, secs, avg_cpu, peak_rss_kb, avg_gpu)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    day,
                    app.app_id,
                    app.secs,
                    app.avg_cpu,
                    app.peak_rss_kb,
                    app.avg_gpu
                ],
            )?;
        }
        Ok(())
    }

    /// Read cached `daily_rollup` rows for `day` (PK `day, app_id`).
    pub fn read_rollup(&self, day: &str) -> Result<Vec<RollupRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT day, app_id, secs, avg_cpu, peak_rss_kb, avg_gpu
             FROM daily_rollup WHERE day = ?1 ORDER BY secs DESC",
        )?;
        let rows = stmt
            .query_map(params![day], |row| {
                Ok(RollupRow {
                    day: row.get(0)?,
                    app_id: row.get(1)?,
                    secs: row.get(2)?,
                    avg_cpu: row.get(3)?,
                    peak_rss_kb: row.get(4)?,
                    avg_gpu: row.get(5)?,
                })
            })?
            .collect::<std::result::Result<_, _>>()?;
        Ok(rows)
    }

    /// Rebuild the DB file to reclaim free pages (run monthly).
    /// Removes no rows — history is kept forever.
    pub fn vacuum(&self) -> Result<()> {
        self.conn.execute_batch("VACUUM;")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `YYYY-MM-DD` + hour -> unix seconds (UTC).
    fn ts(day: &str, hour: u32) -> i64 {
        NaiveDate::parse_from_str(day, "%Y-%m-%d")
            .unwrap()
            .and_hms_opt(hour, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp()
    }

    #[test]
    fn refresh_day_counts_post_midnight_local_events() {
        use chrono::{Local, TimeZone};
        // 00:30 LOCAL — the hour a UTC-midnight day_range silently dropped in
        // UTC+X zones (e.g. 00:00–01:00 WAT is the previous UTC day).
        let store = Store::open_in_memory().unwrap();
        let today = Local::now().date_naive();
        let day = today.format("%Y-%m-%d").to_string();
        let ts_0030 = Local
            .from_local_datetime(&today.and_hms_opt(0, 30, 0).unwrap())
            .single()
            .map(|dt| dt.timestamp())
            .unwrap_or_else(|| {
                // DST spring-forward gap: 01:30 is unambiguously inside the day.
                Local
                    .from_local_datetime(&today.and_hms_opt(1, 30, 0).unwrap())
                    .single()
                    .unwrap()
                    .timestamp()
            });
        store.record_event(ts_0030, "night-owl", "", 1800).unwrap();
        store.refresh_day(&day).unwrap();
        let rows = store.read_rollup(&day).unwrap();
        let row = rows
            .iter()
            .find(|r| r.app_id == "night-owl")
            .expect("00:30 event must land in its local day");
        assert_eq!(row.secs, 1800);
    }

    fn seeded() -> Result<Store> {        let mut store = Store::open_in_memory()?;
        store.batch(|tx| {
            // Mon 2026-01-05 .. Sun 2026-01-11, one event per app per day.
            let days = [
                "2026-01-05",
                "2026-01-06",
                "2026-01-07",
                "2026-01-08",
                "2026-01-09",
                "2026-01-10",
                "2026-01-11",
            ];
            for day in days {
                tx.execute(
                    "INSERT INTO app_events (ts, app_id, title, dur_secs)
                     VALUES (?1, 'firefox', 't', 3600)",
                    params![ts(day, 10)],
                )?;
                tx.execute(
                    "INSERT INTO app_events (ts, app_id, title, dur_secs)
                     VALUES (?1, 'code', 't', 1800)",
                    params![ts(day, 11)],
                )?;
                tx.execute(
                    "INSERT INTO app_events (ts, app_id, title, dur_secs)
                     VALUES (?1, 'kitty', 't', 900)",
                    params![ts(day, 12)],
                )?;
            }
            // Resource samples for one day.
            tx.execute(
                "INSERT INTO resource_samples (ts, app_id, pid, cpu_pct, rss_kb, gpu_pct)
                 VALUES (?1, 'firefox', 1, 10.0, 500000, 5.0)",
                params![ts("2026-01-11", 10)],
            )?;
            tx.execute(
                "INSERT INTO resource_samples (ts, app_id, pid, cpu_pct, rss_kb, gpu_pct)
                 VALUES (?1, 'firefox', 1, 30.0, 700000, 15.0)",
                params![ts("2026-01-11", 11)],
            )?;
            Ok(())
        })?;
        Ok(store)
    }

    #[test]
    fn migrate_is_idempotent_and_stamps_version() {
        let store = Store::open_in_memory().unwrap();
        store.migrate().unwrap();
        store.migrate().unwrap();
        assert_eq!(store.schema_version().unwrap(), "1");
        // All v1 tables exist.
        for table in ["app_events", "resource_samples", "daily_rollup", "meta"] {
            let n: i64 = store
                .conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE name = ?1",
                    params![table],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(n, 1, "missing table {table}");
        }
    }

    #[test]
    fn record_and_daily_total() {
        let store = Store::open_in_memory().unwrap();
        store.record_event(ts("2026-01-05", 9), "firefox", "Example", 120).unwrap();
        store.record_event(ts("2026-01-05", 10), "code", "main.rs", 60).unwrap();
        store.record_event(ts("2026-01-06", 9), "firefox", "Other", 30).unwrap();
        // Plural write-API alias records resource samples too.
        store
            .record_resources(ts("2026-01-05", 9), "firefox", 4242, 12.5, 123456, 3.5)
            .unwrap();
        assert_eq!(store.daily_total_secs("2026-01-05").unwrap(), 180);
        assert_eq!(store.daily_total_secs("2026-01-06").unwrap(), 30);
        assert_eq!(store.daily_total_secs("2026-01-07").unwrap(), 0);
    }

    #[test]
    fn seven_day_stacked_splits_by_category() {
        let store = seeded().unwrap();
        let cats = Categories::builtin();
        let week = store.seven_day_stacked("2026-01-11", &cats).unwrap();
        assert_eq!(week.len(), 7);
        assert_eq!(week[0].day, "2026-01-05");
        assert_eq!(week[6].day, "2026-01-11");
        for day in &week {
            assert_eq!(day.social_secs, 3600); // firefox
            assert_eq!(day.productivity_secs, 1800); // code
            assert_eq!(day.utilities_secs, 900); // kitty
            assert_eq!(day.total_secs, 6300);
        }
    }

    #[test]
    fn per_app_totals_with_resource_aggregates() {
        let store = seeded().unwrap();
        let (start, end) = day_range("2026-01-11").unwrap();
        let apps = store.per_app_totals(start, end).unwrap();
        assert_eq!(apps.len(), 3);
        // Ordered by secs desc: firefox first.
        assert_eq!(apps[0].app_id, "firefox");
        assert_eq!(apps[0].secs, 3600);
        assert!((apps[0].avg_cpu - 20.0).abs() < 1e-9);
        assert_eq!(apps[0].peak_rss_kb, 700000);
        assert!((apps[0].avg_gpu - 10.0).abs() < 1e-9);
        // No samples -> zeroed resources, time still counted.
        let code = apps.iter().find(|a| a.app_id == "code").unwrap();
        assert_eq!(code.secs, 1800);
        assert_eq!(code.avg_cpu, 0.0);
        assert_eq!(code.peak_rss_kb, 0);
    }

    #[test]
    fn week_over_week_delta_pct() {
        let mut store = Store::open_in_memory().unwrap();
        store
            .batch(|tx| {
                for d in 0..7 {
                    let prev = ts("2026-01-05", 0) + d * 86_400;
                    let cur = prev + 7 * 86_400;
                    tx.execute(
                        "INSERT INTO app_events (ts, app_id, title, dur_secs)
                         VALUES (?1, 'a', 't', 100)",
                        params![prev],
                    )?;
                    tx.execute(
                        "INSERT INTO app_events (ts, app_id, title, dur_secs)
                         VALUES (?1, 'a', 't', 200)",
                        params![cur],
                    )?;
                }
                Ok(())
            })
            .unwrap();
        let delta = store.week_over_week("2026-01-18").unwrap();
        assert_eq!(delta.previous_secs, 700);
        assert_eq!(delta.current_secs, 1400);
        assert!((delta.delta_pct.unwrap() - 100.0).abs() < 1e-9);
    }

    #[test]
    fn week_over_week_empty_history() {
        let store = Store::open_in_memory().unwrap();
        let delta = store.week_over_week("2026-01-18").unwrap();
        assert_eq!(delta.delta_pct, Some(0.0));
    }

    #[test]
    fn vacuum_never_deletes_history() {
        let store = Store::open_in_memory().unwrap();
        // Ancient rows (epoch + Y2K) must survive maintenance forever.
        store.record_event(0, "xterm", "old", 42).unwrap();
        store.record_event(946_684_800, "emacs", "older", 7).unwrap();
        store
            .record_resource(0, "xterm", 99, 1.0, 1000, 0.0)
            .unwrap();
        let events_before: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM app_events", [], |r| r.get(0))
            .unwrap();
        let samples_before: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM resource_samples", [], |r| {
                r.get(0)
            })
            .unwrap();
        store.vacuum().unwrap();
        let events_after: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM app_events", [], |r| r.get(0))
            .unwrap();
        let samples_after: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM resource_samples", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!((events_before, samples_before), (2, 1));
        assert_eq!((events_after, samples_after), (2, 1));
        // Ancient seconds still queryable.
        assert_eq!(store.total_secs_range(0, 100).unwrap(), 42);
    }

    #[test]
    fn rollup_refresh_roundtrip() {
        let store = seeded().unwrap();
        store.refresh_daily_rollup("2026-01-11").unwrap();
        // Idempotent: refresh twice, still one row per app.
        store.refresh_daily_rollup("2026-01-11").unwrap();
        let rows = store.read_rollup("2026-01-11").unwrap();
        assert_eq!(rows.len(), 3);
        let ff = rows.iter().find(|r| r.app_id == "firefox").unwrap();
        assert_eq!(ff.secs, 3600);
        assert!((ff.avg_cpu - 20.0).abs() < 1e-9);
        assert_eq!(ff.peak_rss_kb, 700000);
    }

    #[test]
    fn record_batch_writes_events_and_samples_atomically() {
        use crate::model::{AppEvent, ResourceSample};
        let mut store = Store::open_in_memory().unwrap();
        let events = vec![
            AppEvent::new(ts("2026-01-05", 9), "firefox", Some("Example".into()), 120),
            // Redacted title (None) is stored as '' — columns unchanged.
            AppEvent::new(ts("2026-01-05", 10), "steam", None, 60),
        ];
        let samples = vec![
            ResourceSample::new(ts("2026-01-05", 9), "firefox", 4242, 12.5, 123456, 3.5),
            ResourceSample::new(ts("2026-01-05", 10), "steam", 777, 40.0, 900000, 25.0),
        ];
        store.record_batch(&events, &samples).unwrap();
        assert_eq!(store.daily_total_secs("2026-01-05").unwrap(), 180);
        let title: String = store
            .conn
            .query_row(
                "SELECT title FROM app_events WHERE app_id = 'steam'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(title, "");
        let (start, end) = day_range("2026-01-05").unwrap();
        let apps = store.per_app_totals(start, end).unwrap();
        let steam = apps.iter().find(|a| a.app_id == "steam").unwrap();
        assert!((steam.avg_cpu - 40.0).abs() < 1e-9);
        assert_eq!(steam.peak_rss_kb, 900000);
    }

    #[test]
    fn refresh_day_rebuilds_rollup_from_raw_tables() {
        use crate::model::{AppEvent, ResourceSample};
        let mut store = Store::open_in_memory().unwrap();
        store
            .record_batch(
                &[AppEvent::new(ts("2026-01-05", 9), "vlc", None, 300)],
                &[
                    ResourceSample::new(ts("2026-01-05", 9), "vlc", 1, 10.0, 100, 2.0),
                    ResourceSample::new(ts("2026-01-05", 10), "vlc", 1, 30.0, 500, 8.0),
                ],
            )
            .unwrap();
        store.refresh_day("2026-01-05").unwrap();
        let rows = store.read_rollup("2026-01-05").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].secs, 300);
        assert!((rows[0].avg_cpu - 20.0).abs() < 1e-9);
        assert_eq!(rows[0].peak_rss_kb, 500);
        assert!((rows[0].avg_gpu - 5.0).abs() < 1e-9);
    }

    #[test]
    fn seven_day_stacked_splits_gaming_and_media() {
        let mut store = Store::open_in_memory().unwrap();
        store
            .batch(|tx| {
                Store::insert_event(tx, ts("2026-01-11", 10), "steam", "", 3600)?;
                Store::insert_event(tx, ts("2026-01-11", 11), "vlc", "", 1800)?;
                Store::insert_event(tx, ts("2026-01-11", 12), "firefox", "", 900)?;
                Ok(())
            })
            .unwrap();
        let cats = Categories::builtin();
        let week = store.seven_day_stacked("2026-01-11", &cats).unwrap();
        let sat = &week[6];
        assert_eq!(sat.gaming_secs, 3600);
        assert_eq!(sat.media_secs, 1800);
        assert_eq!(sat.social_secs, 900);
        assert_eq!(sat.total_secs, 6300);
        assert_eq!(sat.for_category(Category::Gaming), 3600);
        assert_eq!(sat.for_category(Category::Media), 1800);
    }

    #[test]
    fn batch_helper_commits_atomically() {
        let mut store = Store::open_in_memory().unwrap();
        store
            .batch(|tx| {
                Store::insert_event(tx, ts("2026-01-05", 1), "a", "t", 5)?;
                Store::insert_resource(
                    tx,
                    ts("2026-01-05", 1),
                    "a",
                    1,
                    2.0,
                    3,
                    4.0,
                )?;
                Ok(())
            })
            .unwrap();
        assert_eq!(store.daily_total_secs("2026-01-05").unwrap(), 5);
    }

    #[test]
    fn open_or_create_roundtrips_on_disk() {
        let dir = std::env::temp_dir().join("actwatch-core-test-db");
        let path = dir.join("activity.db");
        let _ = std::fs::remove_dir_all(&dir);
        let store = Store::open_or_create(&path).unwrap();
        let mode: String = store
            .conn
            .query_row("PRAGMA journal_mode;", [], |r| r.get(0))
            .unwrap();
        assert_eq!(mode.to_lowercase(), "wal");
        store.record_event(ts("2026-01-05", 1), "a", "t", 9).unwrap();
        drop(store);
        let store2 = Store::open_or_create(&path).unwrap();
        assert_eq!(store2.daily_total_secs("2026-01-05").unwrap(), 9);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
