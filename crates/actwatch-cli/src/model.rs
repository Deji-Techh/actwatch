//! Data shapes returned by the SQL layer and consumed by the renderers.
//!
//! The v2 contract keeps idle/lock time out of app totals: rows recorded under
//! the reserved `away` app id are pulled out of the app list here, once, so no
//! renderer has to remember the rule.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::core_api::Catalog;

/// Which table a row came from. `daily_rollup` carries the resource columns;
/// `app_events` is live and has no CPU/RAM/GPU detail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Rollup,
    Events,
    Auto,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Rollup => "rollup",
            Source::Events => "events",
            Source::Auto => "auto",
        }
    }
}

impl std::str::FromStr for Source {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "auto" => Ok(Source::Auto),
            "rollup" => Ok(Source::Rollup),
            "events" => Ok(Source::Events),
            other => Err(format!(
                "unknown source `{other}` (expected auto, rollup, events)"
            )),
        }
    }
}

#[derive(Debug, Clone)]
pub struct UsageRow {
    pub day: String,
    pub app_id: String,
    pub secs: i64,
    pub avg_cpu: Option<f64>,
    pub peak_rss_kb: Option<i64>,
    pub avg_gpu: Option<f64>,
    pub source: Source,
}

impl UsageRow {
    /// True for the reserved idle/lock app id.
    pub fn is_away(&self) -> bool {
        Catalog::is_away(&self.app_id)
    }
}

/// One day of app usage. `secs` covers apps only; idle time lives in
/// `away_secs` so shares add up to 100% of *tracked* time.
#[derive(Debug, Clone)]
pub struct DayTotal {
    pub day: String,
    pub secs: i64,
    pub away_secs: i64,
    pub apps: Vec<UsageRow>,
}

impl DayTotal {
    pub fn app_count(&self) -> usize {
        self.apps.len()
    }

    pub fn has_away(&self) -> bool {
        self.away_secs > 0
    }

    /// App time plus idle time: everything recorded for the day.
    pub fn tracked_secs(&self) -> i64 {
        self.secs + self.away_secs
    }

    /// The day's share of the range's tracked time.
    pub fn share_pct(&self, of_secs: i64) -> f64 {
        if of_secs <= 0 {
            0.0
        } else {
            (self.tracked_secs() as f64 / of_secs as f64) * 100.0
        }
    }
}

#[derive(Debug, Clone)]
pub struct RangeReport {
    pub days: Vec<DayTotal>,
}

impl RangeReport {
    /// Splits raw SQL rows into days, pulling reserved `away` rows out of the
    /// app list and folding them into `away_secs`.
    pub fn from_rows(rows: Vec<UsageRow>) -> RangeReport {
        let mut grouped: BTreeMap<String, Vec<UsageRow>> = BTreeMap::new();
        for row in rows {
            grouped.entry(row.day.clone()).or_default().push(row);
        }

        let mut days: Vec<DayTotal> = grouped
            .into_iter()
            .map(|(day, mut rows)| {
                let mut away_secs = 0;
                let mut apps: Vec<UsageRow> = Vec::with_capacity(rows.len());
                for row in rows.drain(..) {
                    if row.is_away() {
                        away_secs += row.secs.max(0);
                    } else {
                        apps.push(row);
                    }
                }
                apps.sort_by(|a, b| {
                    b.secs.cmp(&a.secs).then_with(|| a.app_id.cmp(&b.app_id))
                });
                let secs = apps.iter().map(|a| a.secs).sum();
                DayTotal {
                    day,
                    secs,
                    away_secs,
                    apps,
                }
            })
            .collect();
        days.sort_by(|a, b| b.day.cmp(&a.day));
        RangeReport { days }
    }

    pub fn is_empty(&self) -> bool {
        self.days.is_empty()
    }

    /// App time only: the headline number, never including `away`.
    pub fn total_secs(&self) -> i64 {
        self.days.iter().map(|d| d.secs).sum()
    }

    /// Idle/lock time across the range.
    pub fn away_secs(&self) -> i64 {
        self.days.iter().map(|d| d.away_secs).sum()
    }

    /// Everything recorded: app time plus idle time.
    pub fn tracked_secs(&self) -> i64 {
        self.total_secs() + self.away_secs()
    }

    /// Distinct apps across the whole range. A day-scoped count would
    /// double-count an app seen on several days; `away` is not an app.
    pub fn distinct_apps(&self) -> usize {
        let mut names = BTreeSet::new();
        for day in &self.days {
            for app in &day.apps {
                names.insert(app.app_id.clone());
            }
        }
        names.len()
    }

    pub fn sources(&self) -> Vec<Source> {
        let mut seen: Vec<Source> = Vec::new();
        for day in &self.days {
            for app in &day.apps {
                if !seen.contains(&app.source) {
                    seen.push(app.source);
                }
            }
        }
        seen
    }

    /// Keeps only the apps whose raw id or friendly name contains `needle`
    /// (case-insensitive). Used as the `--app` fallback when the SQL filter on
    /// the raw app id matched nothing, so `--app "Visual Studio Code"` works.
    ///
    /// Idle time is dropped unless the needle names it, otherwise a filter for
    /// some other app would show a day made of nothing but an Away row.
    pub fn retain_apps(&self, needle: &str, catalog: &Catalog) -> RangeReport {
        let needle = needle.to_lowercase();
        let keep_away = needle.contains(crate::core_api::AWAY_APP_ID)
            || needle.contains(crate::core_api::AWAY_LABEL.to_lowercase().as_str());
        let days = self
            .days
            .iter()
            .map(|day| DayTotal {
                day: day.day.clone(),
                secs: day.secs,
                away_secs: if keep_away { day.away_secs } else { 0 },
                apps: day
                    .apps
                    .iter()
                    .filter(|app| {
                        app.app_id.to_lowercase().contains(&needle)
                            || catalog.lookup(&app.app_id).name.to_lowercase().contains(&needle)
                    })
                    .cloned()
                    .collect(),
            })
            .map(|mut day| {
                day.secs = day.apps.iter().map(|a| a.secs).sum();
                day
            })
            .filter(|day| day.app_count() > 0 || day.has_away())
            .collect();
        RangeReport { days }
    }

    /// Drops the CPU/RAM/GPU columns. Resource detail is only present for rows
    /// that came from `daily_rollup`, so mixed ranges export ragged data.
    pub fn slim(self) -> RangeReport {
        RangeReport {
            days: self
                .days
                .into_iter()
                .map(|mut day| {
                    for app in &mut day.apps {
                        app.avg_cpu = None;
                        app.peak_rss_kb = None;
                        app.avg_gpu = None;
                    }
                    day
                })
                .collect(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct JsonUsageRow {
    pub day: String,
    pub app_id: String,
    /// Friendly name from `core::names::DisplayNames`.
    pub name: String,
    /// `null` for away rows: idle is not a category.
    pub category: Option<&'static str>,
    pub secs: i64,
    pub hours: f64,
    pub share_pct: Option<f64>,
    pub away: bool,
    pub source: &'static str,
    pub avg_cpu_pct: Option<f64>,
    pub peak_rss_kb: Option<i64>,
    pub avg_gpu_pct: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct JsonDay {
    pub day: String,
    pub secs: i64,
    pub hours: f64,
    pub share_pct: f64,
    pub apps: usize,
    pub away_secs: i64,
}

#[derive(Debug, Serialize)]
pub struct JsonReport {
    pub tool: &'static str,
    pub version: &'static str,
    pub generated_at: String,
    pub range: JsonRange,
    pub totals: JsonTotals,
    pub days: Vec<JsonDay>,
    pub rows: Vec<JsonUsageRow>,
}

#[derive(Debug, Serialize)]
pub struct JsonRange {
    pub from: String,
    pub to: String,
    pub days: i64,
    pub app: Option<String>,
    pub source: &'static str,
}

#[derive(Debug, Serialize)]
pub struct JsonTotals {
    /// App time, excluding idle. Stable for existing consumers.
    pub total_secs: i64,
    pub hours: f64,
    /// Idle/lock time, reported separately and never folded into the above.
    pub away_secs: i64,
    pub away_hours: f64,
    pub days: i64,
    pub apps: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(day: &str, app: &str, secs: i64) -> UsageRow {
        UsageRow {
            day: day.into(),
            app_id: app.into(),
            secs,
            avg_cpu: Some(3.5),
            peak_rss_kb: Some(1_500_000),
            avg_gpu: None,
            source: Source::Rollup,
        }
    }

    #[test]
    fn away_is_kept_out_of_app_totals() {
        let r = RangeReport::from_rows(vec![
            row("2026-10-10", "firefox", 3600),
            row("2026-10-10", "code", 1800),
            row("2026-10-10", "away", 900),
        ]);
        assert_eq!(r.total_secs(), 5400);
        assert_eq!(r.away_secs(), 900);
        assert_eq!(r.tracked_secs(), 6300);
        assert_eq!(r.distinct_apps(), 2);
        assert!(r.days[0].has_away());
        let day = &r.days[0];
        assert_eq!(day.app_count(), 2);
        assert_eq!(day.away_secs, 900);
        assert_eq!(day.tracked_secs(), 6300);
    }

    #[test]
    fn report_without_away_behaves_like_v1() {
        let r = RangeReport::from_rows(vec![
            row("2026-10-10", "firefox", 3600),
            row("2026-10-09", "firefox", 600),
        ]);
        assert_eq!(r.total_secs(), 4200);
        assert_eq!(r.away_secs(), 0);
        assert!(!r.days[0].has_away());
        assert_eq!(r.distinct_apps(), 1);
        assert!((r.days[0].share_pct(4200) - 85.7142857).abs() < 0.01);
    }

    #[test]
    fn day_share_uses_tracked_time() {
        let r = RangeReport::from_rows(vec![
            row("2026-10-10", "firefox", 3600),
            row("2026-10-10", "away", 1800),
        ]);
        // Idle counts towards the day's share of the range…
        assert!((r.days[0].share_pct(r.tracked_secs()) - 100.0).abs() < 0.001);
        // …but not towards the app share of the day.
        assert_eq!(r.days[0].tracked_secs(), 5400);
        assert_eq!(r.days[0].share_pct(0), 0.0);
    }

    #[test]
    fn source_selection() {
        assert_eq!("auto".parse::<Source>().unwrap(), Source::Auto);
        assert_eq!("rollup".parse::<Source>().unwrap(), Source::Rollup);
        assert_eq!("events".parse::<Source>().unwrap(), Source::Events);
        assert!("nope".parse::<Source>().is_err());
    }

    #[test]
    fn retain_apps_matches_friendly_names() {
        let r = RangeReport::from_rows(vec![
            row("2026-10-10", "code", 3600),
            row("2026-10-10", "mpv", 600),
            row("2026-10-10", "away", 900),
        ]);
        let catalog = Catalog::from_toml_str("[aliases]\ncode = \"Visual Studio Code\"\n");
        let filtered = r.retain_apps("studio", &catalog);
        assert_eq!(filtered.total_secs(), 3600);
        assert_eq!(filtered.distinct_apps(), 1);
        // Idle is not shown for a filter that did not ask for it.
        assert_eq!(filtered.away_secs(), 0);

        // Matching the raw id still works.
        assert_eq!(r.retain_apps("mpv", &catalog).total_secs(), 600);
        assert!(r.retain_apps("nothing-matches", &catalog).is_empty());
    }
}