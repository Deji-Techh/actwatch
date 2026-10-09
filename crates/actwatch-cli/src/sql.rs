//! Every SQL statement executed by the `actwatch` CLI lives in this module.
//!
//! The CLI is strictly read-only: it opens the daemon database with
//! `SQLITE_OPEN_READ_ONLY` and additionally pins `PRAGMA query_only`.
//! It never creates, migrates, vacuums or prunes anything.
//!
//! Schema reference (docs/ARCHITECTURE.md, v1):
//!   app_events(ts, app_id, title, dur_secs)
//!   resource_samples(ts, app_id, pid, cpu_pct, rss_kb, gpu_pct)
//!   daily_rollup(day, app_id, secs, avg_cpu, peak_rss_kb, avg_gpu)
//!   meta(key, value)

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

const ROLLUP_TOTALS: &str = "SELECT COUNT(DISTINCT day), MIN(day), MAX(day), COALESCE(SUM(secs), 0)
     FROM daily_rollup";

const EVENTS_TOTALS: &str = "SELECT COUNT(DISTINCT date(ts, 'unixepoch', 'localtime')),
            MIN(date(ts, 'unixepoch', 'localtime')),
            MAX(date(ts, 'unixepoch', 'localtime')),
            COALESCE(SUM(dur_secs), 0)
     FROM app_events";

/// Builds a LIKE pattern that matches `needle` literally, so an app called
/// `100%_free` does not silently turn into a wildcard query.
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
    let pattern = app.map(like_pattern);

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
    pub total_secs: i64,
}

impl HistoryTotals {
    fn merge(mut self, other: HistoryTotals) -> HistoryTotals {
        self.days += other.days;
        self.total_secs += other.total_secs;
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
    let mut rows = stmt.query([])?;
    let row = rows.next()?.context("totals query returned no rows")?;
    Ok(HistoryTotals {
        days: row.get(0)?,
        first_day: row.get(1)?,
        last_day: row.get(2)?,
        total_secs: row.get::<_, i64>(3)?.max(0),
    })
}
