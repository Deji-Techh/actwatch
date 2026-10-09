//! Human-readable rendering plus the CSV/JSON serialisation used by `export`.

use std::fmt::Write as _;

use crate::date::DateRange;
use crate::model::{
    JsonDay, JsonRange, JsonReport, JsonTotals, JsonUsageRow, RangeReport, Source, UsageRow,
};

pub const TOOL: &str = "actwatch";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

const BAR_FULL: char = '\u{2588}';
const BAR_EMPTY: char = '\u{2591}';

/// `6h 23m`, `45m 12s`, `15m`, `0s`. Seconds are only shown when they carry
/// information, so whole-minute durations stay short.
pub fn human_duration(secs: i64) -> String {
    let secs = secs.max(0);
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    match (h, m, s) {
        (0, 0, 0) => "0s".into(),
        (0, 0, _) => format!("{s}s"),
        (0, m, 0) => format!("{m}m"),
        (0, m, _) => format!("{m}m {s}s"),
        (h, 0, 0) => format!("{h}h"),
        (h, m, 0) => format!("{h}h {m}m"),
        (h, m, _) => format!("{h}h {m}m {s}s"),
    }
}

pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn human_rss(kb: Option<i64>) -> String {
    match kb {
        Some(kb) if kb > 0 => human_bytes(kb as u64),
        _ => "-".into(),
    }
}

fn human_pct(value: Option<f64>) -> String {
    match value {
        Some(v) if v > 0.0 => format!("{v:.1}%"),
        _ => "-".into(),
    }
}

fn bar(fraction: f64, width: usize) -> String {
    if !fraction.is_finite() || fraction <= 0.0 {
        return BAR_EMPTY.to_string().repeat(width);
    }
    let filled = ((fraction * width as f64).round() as usize).min(width);
    let mut out = String::with_capacity(filled * 3);
    for _ in 0..filled {
        out.push(BAR_FULL);
    }
    for _ in filled..width {
        out.push(BAR_EMPTY);
    }
    out
}

fn section(rule: char, text: &str) -> String {
    format!("{text}\n{}", rule.to_string().repeat(72))
}

/// The default `query` view: one block per day, apps sorted by time.
pub fn render_table(report: &RangeReport, range: &DateRange, limit: usize) -> String {
    if report.is_empty() {
        return format!(
            "actwatch — no tracked activity for {} ({})\n",
            range.label(),
            source_hint(range)
        );
    }

    let mut out = String::new();
    let total = report.total_secs();
    let _ = writeln!(
        out,
        "actwatch — {} — {} tracked across {} app{} · {}",
        range.label(),
        human_duration(total),
        report.distinct_apps(),
        if report.distinct_apps() == 1 { "" } else { "s" },
        source_hint(range)
    );
    let _ = writeln!(out);

    for day in &report.days {
        let _ = writeln!(
            out,
            "{}  {} tracked · {} app{} · {:.0}% of a {} day",
            day.day,
            human_duration(day.secs),
            day.app_count(),
            if day.app_count() == 1 { "" } else { "s" },
            day.share_pct(total),
            human_duration(86_400)
        );
        let _ = writeln!(
            out,
            "  {:<28} {:>9} {:>6}  {:>6} {:>8} {:>6}  share of range",
            "app", "time", "share", "cpu", "peak rss", "gpu"
        );

        let width = day
            .apps
            .iter()
            .map(|a| a.app_id.chars().count())
            .max()
            .unwrap_or(4)
            .clamp(4, 28);

        let shown: Vec<&UsageRow> = if limit == 0 {
            day.apps.iter().collect()
        } else {
            day.apps.iter().take(limit).collect()
        };

        for app in shown {
            let share = if total > 0 {
                (app.secs as f64 / total as f64) * 100.0
            } else {
                0.0
            };
            let _ = writeln!(
                out,
                "  {:<width$} {:>9} {:>5.1}%  {:>6} {:>8} {:>6}  {}",
                app.app_id,
                human_duration(app.secs),
                share,
                human_pct(app.avg_cpu),
                human_rss(app.peak_rss_kb),
                human_pct(app.avg_gpu),
                bar(share / 100.0, 16),
                width = width
            );
        }

        if limit > 0 && day.app_count() > limit {
            let hidden = day.app_count() - limit;
            let _ = writeln!(
                out,
                "  … {} more app{} (use --limit 0 to show all, or `actwatch export --csv`)",
                hidden,
                if hidden == 1 { "" } else { "s" }
            );
        }
        let _ = writeln!(out);
    }

    out.trim_end().to_string()
}

fn source_hint(range: &DateRange) -> String {
    match range.source {
        Source::Auto => "rollup + live events".into(),
        other => format!("{} only", other.as_str()),
    }
}

fn csv_escape(value: &str) -> String {
    if value.contains(['"', ',', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

fn csv_num(value: Option<f64>) -> String {
    match value {
        Some(v) => format!("{v:.2}"),
        None => String::new(),
    }
}

/// RFC 4180 style CSV. `secs` is the canonical column; the rest are derived.
pub fn render_csv(report: &RangeReport) -> String {
    let mut out = String::from(
        "day,app_id,secs,hours,share_pct,avg_cpu_pct,peak_rss_kb,avg_gpu_pct,source\n",
    );
    let total = report.total_secs();
    for day in &report.days {
        for app in &day.apps {
            let share = if total > 0 {
                (app.secs as f64 / total as f64) * 100.0
            } else {
                0.0
            };
            let _ = writeln!(
                out,
                "{},{},{},{:.4},{:.2},{},{},{},{}",
                csv_escape(&app.day),
                csv_escape(&app.app_id),
                app.secs,
                app.secs as f64 / 3600.0,
                share,
                csv_num(app.avg_cpu),
                app.peak_rss_kb.map(|v| v.to_string()).unwrap_or_default(),
                csv_num(app.avg_gpu),
                app.source.as_str()
            );
        }
    }
    out.trim_end().to_string()
}

/// Stable machine-readable payload for panels, scripts and the caelestia hook.
pub fn render_json(report: &RangeReport, range: &DateRange) -> Result<String, serde_json::Error> {
    let total = report.total_secs();
    let mut rows = Vec::new();
    let mut days = Vec::new();

    for day in &report.days {
        days.push(JsonDay {
            day: day.day.clone(),
            secs: day.secs,
            hours: day.secs as f64 / 3600.0,
            share_pct: day.share_pct(total),
            apps: day.app_count(),
        });
        for app in &day.apps {
            rows.push(JsonUsageRow {
                day: app.day.clone(),
                app_id: app.app_id.clone(),
                secs: app.secs,
                hours: app.secs as f64 / 3600.0,
                source: app.source.as_str(),
                avg_cpu_pct: app.avg_cpu,
                peak_rss_kb: app.peak_rss_kb,
                avg_gpu_pct: app.avg_gpu,
            });
        }
    }

    let payload = JsonReport {
        tool: TOOL,
        version: VERSION,
        generated_at: chrono::Local::now()
            .format("%Y-%m-%dT%H:%M:%S%:z")
            .to_string(),
        range: JsonRange {
            from: range.start_day(),
            to: range.end_day(),
            days: range.span_days(),
            app: range.app.clone(),
            source: range.source.as_str(),
        },
        totals: JsonTotals {
            total_secs: total,
            hours: total as f64 / 3600.0,
            days: report.days.len() as i64,
            apps: report.distinct_apps(),
        },
        days,
        rows,
    };
    serde_json::to_string_pretty(&payload)
}

/// `status` renders as an aligned key/value block.
pub fn render_pairs(rows: &[(&str, String)]) -> String {
    let width = rows
        .iter()
        .map(|(k, _)| k.chars().count())
        .max()
        .unwrap_or(0);
    let mut out = String::new();
    for (key, value) in rows {
        let _ = writeln!(out, "{:<width$}  {}", key, value, width = width);
    }
    out.trim_end().to_string()
}

pub fn heading(text: &str) -> String {
    section('\u{2500}', text)
}

pub fn warning(text: &str) -> String {
    format!("warning: {text}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{DayTotal, UsageRow};

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

    fn report(rows: Vec<UsageRow>) -> RangeReport {
        RangeReport::from_rows(rows)
    }

    #[test]
    fn durations_read_naturally() {
        assert_eq!(human_duration(0), "0s");
        assert_eq!(human_duration(45), "45s");
        assert_eq!(human_duration(900), "15m");
        assert_eq!(human_duration(3600), "1h");
        assert_eq!(human_duration(3600 + 180), "1h 3m");
        assert_eq!(human_duration(45 * 60 + 12), "45m 12s");
    }

    #[test]
    fn bytes_are_human_scaled() {
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(2048), "2.0 KiB");
        assert_eq!(human_bytes(5 * 1024 * 1024), "5.0 MiB");
    }

    #[test]
    fn csv_quotes_hostile_app_names() {
        let r = report(vec![row("2026-10-10", "my,app", 60)]);
        let csv = render_csv(&r);
        assert!(csv.contains("\"my,app\""));
        assert!(csv.starts_with("day,app_id,secs"));
    }

    #[test]
    fn json_roundtrips_totals() {
        let r = report(vec![row("2026-10-10", "firefox", 3600)]);
        let json = render_json(&r, &range()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["totals"]["total_secs"], 3600);
        assert_eq!(v["rows"][0]["app_id"], "firefox");
    }

    #[test]
    fn groups_by_day_and_sorts_desc() {
        let r = report(vec![
            row("2026-10-09", "a", 60),
            row("2026-10-10", "b", 120),
            row("2026-10-10", "c", 300),
        ]);
        assert_eq!(r.days.len(), 2);
        assert_eq!(r.days[0].day, "2026-10-10");
        assert_eq!(r.days[0].apps[0].app_id, "c");
        assert_eq!(r.total_secs(), 480);
    }

    #[test]
    fn empty_report_renders_hint() {
        let r = report(vec![]);
        assert!(render_table(&r, &range(), 0).contains("no tracked activity"));
        assert!(render_csv(&r).starts_with("day,"));
    }

    #[test]
    fn limit_truncates_with_notice() {
        let r = report(vec![
            row("2026-10-10", "a", 300),
            row("2026-10-10", "b", 200),
        ]);
        let out = render_table(&r, &range(), 1);
        assert!(out.contains("1 more app"));
        let full = render_table(&r, &range(), 0);
        assert!(!full.contains("more app"));
    }

    #[test]
    fn day_totals_share_percentage() {
        let d = DayTotal {
            day: "2026-10-10".into(),
            secs: 3600,
            apps: vec![row("2026-10-10", "a", 3600)],
        };
        assert!((d.share_pct(14400) - 25.0).abs() < 0.001);
        assert_eq!(d.share_pct(0), 0.0);
    }

    fn range() -> DateRange {
        let d = chrono::NaiveDate::from_ymd_opt(2026, 10, 10).unwrap();
        DateRange {
            from: d,
            to: d,
            app: None,
            source: Source::Rollup,
        }
    }
}
