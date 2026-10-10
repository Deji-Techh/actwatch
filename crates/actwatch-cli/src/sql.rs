//! Every SQL statement executed by the `actwatch` CLI lives in this module.
//!
//! The CLI is strictly read-only: it opens the daemon database with
//! `SQLITE_OPEN_READ_ONLY` and additionally pins `PRAGMA query_only`.
//! It never creates, migrates, vacuums or prunes anything.
//!
//! Schema reference (docs/ARCHITECTURE.md):
//!   app_events(ts, app_id, title, dur_secs)
//!   resource_samples(ts, app_id, pid, cpu_pct, rss_kb, gpu_pct)
//!   daily_rollup(day, app_id, secs, avg_cpu, peak_rss_kb, avg_gpu)
//!   meta(key, value)
//!
//! Read contract (docs/ARCHITECTURE.md, "Reading the database"): `--source auto`
//! reads completed days from `daily_rollup` — the cheap source that carries the
//! CPU/RAM/GPU columns — and today live from `app_events`, so the current day
//! is never stale. Every row records which table produced it.
//!
//! Why the rollup read is not `core::Store::read_rollup`: `Store` opens the
//! database read-write (`Connection::open`, `journal_mode=WAL`, `migrate()`).
//! A read-only frontend cannot use it without giving up `query_only`. The
//! v2 contract therefore needs a read-only rollup reader in `actwatch-core`
//! (e.g. `Store::open_read_only` + `read_rollup`); until it exists these two
//! statements stay here, in the one module the read contract is audited in.
//! See docs/ARCHITECTURE.md, "CLI v2 status".

use anyhow::{Context, Result};
use rusqlite::{params, Connection, Row};

use crate::model::{RangeReport, Source, UsageRow};

/// Tables the CLI knows how to read. Used for the presence check in `status`.
pub const KNOWN_TABLES: [&str; 4] = ["app_events", "resource_samples", "daily_rollup", "meta"];

/// `day`/`app_id` pairs inside `daily_rollup`, aggregated over an inclusive
/// day range and optionally filtered by app. `?3` is a LIKE pattern or NULL.
const ROLLUP_RANGE: &str = "SELECT day,
            app_id,
            SUM(secs)        AS secs,
            AVG(avg_cpu)     AS avg_cpu,
            MAX(peak_rss_kb) AS peak_rss_kb,
            AVG(avg_gpu)     AS avg_gpu
     FROM daily_rollup
     WHERE day >= ?1 AND day <= ?2
       AND (?3 IS NULL OR app_id LIKE ?3 ESCAPE '\\')
     GROUP BY day, app_id
     ORDER BY day DESC, secs DESC";

/// The same shape derived from raw activation events. `?1`/`?2` are unix
/// seconds (inclusive, exclusive). Resource columns are not available at this
/// granularity because they are only aggregated into `daily_rollup`.
const EVENTS_RANGE: &str = "SELECT date(ts, 'unixepoch', 'localtime') AS day,
            app_id,
            SUM(dur_secs)   AS secs
     FROM app_events
     WHERE ts >= ?1 AND ts < ?2
       AND (?3 IS NULL OR app_id LIKE ?3 ESCAPE '\\')
     GROUP BY day, app_id
     ORDER BY day DESC, secs DESC";

const TABLES_PRESENT: &str = "SELECT name
     FROM sqlite_master
     WHERE type = 'table' AND name IN (?1, ?2, ?3, ?4)";

const META_GET: &str = "SELECT value FROM meta WHERE key = ?1 LIMIT 1";

/// Idle time is recorded under the reserved `away` app id, so totals keep
/// app time and away time in separate columns.
const AWAY_ID: &str = crate::core_api::AWAY_APP_ID;

const ROLLUP_TOTALS: &str = "SELECT COUNT(DISTINCT day), MIN(day), MAX(day),
            COALESCE(SUM(CASE WHEN app_id <> ?1 THEN secs ELSE 0 END), 0),
            COALESCE(SUM(CASE WHEN app_id = ?1 THEN secs ELSE 0 END), 0)
     FROM daily_rollup";

const EVENTS_TOTALS: &str = "SELECT COUNT(DISTINCT date(ts, 'unixepoch', 'localtime')),
            MIN(date(ts, 'unixepoch', 'localtime')),
            MAX(date(ts, 'unixepoch', 'localtime')),
            COALESCE(SUM(CASE WHEN app_id <> ?1 THEN dur_secs ELSE 0 END), 0),
            COALESCE(SUM(CASE WHEN app_id = ?1 THEN dur_secs ELSE 0 END), 0)
     FROM app_events";

/// Builds a LIKE pattern that contains `needle` but matches it literally, so
/// `--app 100%_free` does not silently turn into a wildcard query.
fn like_contains(needle: &str) -> String {
    format!("%{}%", like_pattern(needle))
}

/// Escapes the LIKE metacharacters in `needle`.
fn like_pattern(needle: &str) -> String {
    let mut out = String::with_capacity(needle.len());
    for ch in needle.chars() {
        if matches!(ch, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

fn map_rollup_row(row: &Row<'_>) -> rusqlite::Result<UsageRow> {
    Ok(UsageRow {
        day: row.get::<_, String>(0)?,
        app_id: row.get::<_, String>(1)?,
        secs: row.get::<_, i64>(2)?.max(0),
        avg_cpu: row.get::<_, Option<f64>>(3)?,
        peak_rss_kb: row.get::<_, Option<i64>>(4)?,
        avg_gpu: row.get::<_, Option<f64>>(5)?,
        source: Source::Rollup,
    })
}

fn map_events_row(row: &Row<'_>) -> rusqlite::Result<UsageRow> {
    Ok(UsageRow {
        day: row.get::<_, String>(0)?,
        app_id: row.get::<_, String>(1)?,
        secs: row.get::<_, i64>(2)?.max(0),
        avg_cpu: None,
        peak_rss_kb: None,
        avg_gpu: None,
        source: Source::Events,
    })
}

/// Reads rows over an inclusive `YYYY-MM-DD` range from one of the two sources.
pub fn fetch_range(
    conn: &Connection,
    from: &str,
    to: &str,
    app: Option<&str>,
    source: Source,
) -> Result<Vec<UsageRow>> {
    let pattern = app.map(like_contains);

    let rows = match source {
        Source::Rollup => {
            let mut stmt = conn.prepare_cached(ROLLUP_RANGE)?;
            let rows = stmt.query(params![from, to, pattern])?;
            collect(rows, map_rollup_row)?
        }
        Source::Events => {
            let start = crate::date::day_start_epoch(from)?;
            let end = crate::date::day_start_epoch_after(to)?;
            let mut stmt = conn.prepare_cached(EVENTS_RANGE)?;
            let rows = stmt.query(params![start, end, pattern])?;
            collect(rows, map_events_row)?
        }
        Source::Auto => unreachable!("auto is resolved by build_report"),
    };
    Ok(rows)
}

fn collect<T>(
    mut rows: rusqlite::Rows<'_>,
    map: fn(&Row<'_>) -> rusqlite::Result<T>,
) -> Result<Vec<T>> {
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        out.push(map(row).context("failed to read row")?);
    }
    Ok(out)
}

/// Builds a report over the inclusive `YYYY-MM-DD` range `[from, to]`.
///
/// `Source::Auto` mixes both tables on purpose: `daily_rollup` is the cheap,
/// resource-annotated source but lags for the current day, so today is always
/// read live from `app_events`. Every row records which table produced it.
pub fn build_report(
    conn: &Connection,
    from: &str,
    to: &str,
    today: &str,
    app: Option<&str>,
    source: Source,
) -> Result<RangeReport> {
    let rows = match source {
        Source::Rollup => fetch_range(conn, from, to, app, Source::Rollup)?,
        Source::Events => fetch_range(conn, from, to, app, Source::Events)?,
        Source::Auto => {
            let mut rows = Vec::new();
            if to < today {
                rows.extend(fetch_range(conn, from, to, app, Source::Rollup)?);
            } else if from > today {
                rows.extend(fetch_range(conn, from, to, app, Source::Events)?);
            } else {
                rows.extend(fetch_range(
                    conn,
                    from,
                    &prev_day(today)?,
                    app,
                    Source::Rollup,
                )?);
                rows.extend(fetch_range(conn, today, to, app, Source::Events)?);
            }
            rows
        }
    };

    Ok(RangeReport::from_rows(rows))
}

fn prev_day(day: &str) -> Result<String> {
    let d = crate::date::parse_day(day)?;
    d.pred_opt()
        .map(|p| p.format(crate::date::DAY_FMT).to_string())
        .with_context(|| format!("date underflows at {day}"))
}

/// Table names that actually exist in the given database.
pub fn tables_present(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(TABLES_PRESENT)?;
    let names = stmt
        .query_map(
            params![
                KNOWN_TABLES[0],
                KNOWN_TABLES[1],
                KNOWN_TABLES[2],
                KNOWN_TABLES[3]
            ],
            |row| row.get::<_, String>(0),
        )?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("failed to list tables")?;
    Ok(names)
}

pub fn meta_get(conn: &Connection, key: &str) -> Result<Option<String>> {
    let mut stmt = conn.prepare_cached(META_GET)?;
    let mut rows = stmt.query(params![key])?;
    match rows.next()? {
        Some(row) => Ok(Some(row.get::<_, String>(0)?)),
        None => Ok(None),
    }
}

#[derive(Debug, Default, Clone)]
pub struct HistoryTotals {
    pub days: i64,
    pub first_day: Option<String>,
    pub last_day: Option<String>,
    /// App time, `away` excluded.
    pub total_secs: i64,
    /// Idle/lock time recorded under the reserved `away` id.
    pub away_secs: i64,
}

impl HistoryTotals {
    fn merge(mut self, other: HistoryTotals) -> HistoryTotals {
        self.days += other.days;
        self.total_secs += other.total_secs;
        self.away_secs += other.away_secs;
        self.first_day = earliest(self.first_day.as_ref(), other.first_day.as_ref());
        self.last_day = latest(self.last_day.as_ref(), other.last_day.as_ref());
        self
    }
}

fn earliest(a: Option<&String>, b: Option<&String>) -> Option<String> {
    match (a, b) {
        (Some(x), Some(y)) => Some(if x <= y { x.clone() } else { y.clone() }),
        (Some(x), None) | (None, Some(x)) => Some(x.clone()),
        (None, None) => None,
    }
}

fn latest(a: Option<&String>, b: Option<&String>) -> Option<String> {
    match (a, b) {
        (Some(x), Some(y)) => Some(if x >= y { x.clone() } else { y.clone() }),
        (Some(x), None) | (None, Some(x)) => Some(x.clone()),
        (None, None) => None,
    }
}

/// Whole-history totals, unioned over whichever of the two tables exist.
pub fn history_totals(conn: &Connection, present: &[String]) -> Result<HistoryTotals> {
    let mut total = HistoryTotals::default();
    if present.iter().any(|t| t == "daily_rollup") {
        total = total.merge(totals_from(conn, ROLLUP_TOTALS)?);
    }
    if present.iter().any(|t| t == "app_events") {
        total = total.merge(totals_from(conn, EVENTS_TOTALS)?);
    }
    Ok(total)
}

fn totals_from(conn: &Connection, sql: &str) -> Result<HistoryTotals> {
    let mut stmt = conn.prepare_cached(sql)?;
    let mut rows = stmt.query(params![AWAY_ID])?;
    let row = rows.next()?.context("totals query returned no rows")?;
    Ok(HistoryTotals {
        days: row.get(0)?,
        first_day: row.get(1)?,
        last_day: row.get(2)?,
        total_secs: row.get::<_, i64>(3)?.max(0),
        away_secs: row.get::<_, i64>(4)?.max(0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::date;
    use actwatch_core::SCHEMA_SQL;

    /// Yesterday's day is in the past, so `--source auto` must take it from
    /// `daily_rollup`; today must come from `app_events`.
    fn seeded() -> (Connection, String, String) {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_SQL).unwrap();

        let today = date::today();
        let yesterday = today.pred_opt().unwrap();
        let today_s = date::format_day(&today);
        let yesterday_s = date::format_day(&yesterday);
        let yesterday_ts = date::day_start_epoch(&yesterday_s).unwrap();
        let today_ts = date::day_start_epoch(&today_s).unwrap();

        // Yesterday exists only as a rollup row (the daemon checkpointed it).
        conn.execute(
            "INSERT INTO daily_rollup (day, app_id, secs, avg_cpu, peak_rss_kb, avg_gpu)
             VALUES (?1, 'firefox', 3600, 4.0, 900000, 1.5)",
            params![yesterday_s],
        )
        .unwrap();
        // Today has a *stale* rollup row that must not win over live events.
        conn.execute(
            "INSERT INTO daily_rollup (day, app_id, secs, avg_cpu, peak_rss_kb, avg_gpu)
             VALUES (?1, 'firefox', 99999, 99.0, 1, 99.0)",
            params![today_s],
        )
        .unwrap();

        for (ts, app_id, secs) in [
            (yesterday_ts, "code", 30),
            (today_ts, "firefox", 600),
            (today_ts + 120, "away", 900),
        ] {
            conn.execute(
                "INSERT INTO app_events (ts, app_id, title, dur_secs) VALUES (?1, ?2, '', ?3)",
                params![ts, app_id, secs],
            )
            .unwrap();
        }
        (conn, yesterday_s, today_s)
    }

    #[test]
    fn auto_reads_past_days_from_rollup_and_today_from_events() {
        let (conn, yesterday, today) = seeded();
        let report = build_report(&conn, &yesterday, &today, &today, None, Source::Auto).unwrap();
        assert_eq!(report.days.len(), 2);

        let today_row = report.days.iter().find(|d| d.day == today).unwrap();
        let today_app = today_row.apps.iter().find(|a| a.app_id == "firefox").unwrap();
        assert_eq!(today_app.source, Source::Events);
        // The stale rollup row for today is ignored.
        assert_eq!(today_app.secs, 600);
        // Live rows carry no resource columns.
        assert!(today_app.avg_cpu.is_none());
        assert_eq!(today_app.peak_rss_kb, None);

        let yesterday_row = report.days.iter().find(|d| d.day == yesterday).unwrap();
        let rollup_app = yesterday_row.apps.iter().find(|a| a.app_id == "firefox").unwrap();
        assert_eq!(rollup_app.source, Source::Rollup);
        assert_eq!(rollup_app.secs, 3600);
        assert_eq!(rollup_app.avg_cpu, Some(4.0));
        assert_eq!(rollup_app.peak_rss_kb, Some(900_000));

        // The sources are both present, which is what `auto` means.
        let sources = report.sources();
        assert!(sources.contains(&Source::Events));
        assert!(sources.contains(&Source::Rollup));
    }

    #[test]
    fn auto_uses_events_for_a_future_only_range() {
        let (conn, _yesterday, today) = seeded();
        let tomorrow = date::parse_day(&today)
            .unwrap()
            .succ_opt()
            .unwrap()
            .format(date::DAY_FMT)
            .to_string();
        let report =
            build_report(&conn, &tomorrow, &tomorrow, &today, None, Source::Auto).unwrap();
        assert!(report.is_empty());
    }

    #[test]
    fn away_stays_out_of_app_totals() {
        let (conn, _yesterday, today) = seeded();
        let report =
            build_report(&conn, &today, &today, &today, None, Source::Auto).unwrap();
        let day = &report.days[0];
        assert_eq!(day.app_count(), 1);
        assert_eq!(day.away_secs, 900);
        assert_eq!(report.total_secs(), 600);
        assert_eq!(report.away_secs(), 900);
    }

    #[test]
    fn history_totals_split_away_from_app_time() {
        let (conn, yesterday, today) = seeded();
        let present = tables_present(&conn).unwrap();
        let totals = history_totals(&conn, &present).unwrap();
        assert_eq!(totals.first_day.as_deref(), Some(yesterday.as_str()));
        assert_eq!(totals.last_day.as_deref(), Some(today.as_str()));
        // Rollup (3600 + 99999) + events (30 + 600), away reported apart.
        // App time excludes the 900s of idle recorded as `away`.
        assert_eq!(totals.total_secs, 3600 + 99999 + 30 + 600);
        assert_eq!(totals.away_secs, 900);
    }

    #[test]
    fn app_filter_is_a_literal_substring_match() {
        let (conn, _, today) = seeded();
        // `--app` is a substring match.
        let rows = fetch_range(&conn, &today, &today, Some("fire"), Source::Events).unwrap();
        assert_eq!(rows.len(), 1);
        // …but LIKE metacharacters in the needle stay literal.
        let rows = fetch_range(&conn, &today, &today, Some("fir%"), Source::Events).unwrap();
        assert!(rows.is_empty(), "LIKE wildcards must not leak in");
        assert_eq!(like_contains("a_b"), "%a\\_b%");
    }
}
