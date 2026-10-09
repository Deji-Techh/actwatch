//! Data shapes returned by the SQL layer and consumed by the renderers.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

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

#[derive(Debug, Clone)]
pub struct DayTotal {
    pub day: String,
    pub secs: i64,
    pub apps: Vec<UsageRow>,
}

impl DayTotal {
    pub fn app_count(&self) -> usize {
        self.apps.len()
    }

    pub fn share_pct(&self, of_secs: i64) -> f64 {
        if of_secs <= 0 {
            0.0
        } else {
            (self.secs as f64 / of_secs as f64) * 100.0
        }
    }
}

#[derive(Debug, Clone)]
pub struct RangeReport {
    pub days: Vec<DayTotal>,
}

impl RangeReport {
    pub fn from_rows(rows: Vec<UsageRow>) -> RangeReport {
        let mut grouped: BTreeMap<String, Vec<UsageRow>> = BTreeMap::new();
        for row in rows {
            grouped.entry(row.day.clone()).or_default().push(row);
        }

        let mut days: Vec<DayTotal> = grouped
            .into_iter()
            .map(|(day, mut apps)| {
                apps.sort_by(|a, b| b.secs.cmp(&a.secs).then_with(|| a.app_id.cmp(&b.app_id)));
                let secs = apps.iter().map(|a| a.secs).sum();
                DayTotal { day, secs, apps }
            })
            .collect();
        days.sort_by(|a, b| b.day.cmp(&a.day));
        RangeReport { days }
    }

    pub fn is_empty(&self) -> bool {
        self.days.is_empty()
    }

    pub fn total_secs(&self) -> i64 {
        self.days.iter().map(|d| d.secs).sum()
    }

    /// Distinct app names across the whole range. A day-scoped count would
    /// double-count an app seen on several days.
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
    pub secs: i64,
    pub hours: f64,
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
    pub total_secs: i64,
    pub hours: f64,
    pub days: i64,
    pub apps: usize,
}
