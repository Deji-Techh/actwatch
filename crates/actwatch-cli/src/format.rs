//! Human-readable rendering plus the CSV/JSON serialisation used by `export`.
//!
//! Output is plain monochrome: no ANSI escapes, no colour, no glyphs that
//! need a font feature. Row labels come from [`crate::core_api::Catalog`], so
//! the CLI never invents a name or a category, and idle time (`away`) is
//! rendered as its own row instead of being folded into the app list.

use std::fmt::Write as _;

use crate::core_api::Catalog;
use crate::date::DateRange;
use crate::model::{
    JsonDay, JsonRange, JsonReport, JsonTotals, JsonUsageRow, RangeReport, Source, UsageRow,
};

pub const TOOL: &str = "actwatch";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

const BAR_FULL: char = '\u{2588}';
const BAR_EMPTY: char = '\u{2591}';
const RULE: char = '\u{2500}';
const BAR_WIDTH: usize = 16;
const NAME_WIDTH_MAX: usize = 28;
const CATEGORY_WIDTH: usize = 22;
const AWAY_NOTE: &str = "idle, excluded from app totals";

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

fn plural(count: usize, word: &str) -> String {
    if count == 1 {
        format!("{count} {word}")
    } else {
        format!("{count} {word}s")
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

/// The default `query` view: one block per day, apps sorted by time, idle
/// time on its own row. Monochrome only — no ANSI colour anywhere.
pub fn render_table(
    report: &RangeReport,
    range: &DateRange,
    limit: usize,
    catalog: &Catalog,
) -> String {
    if report.is_empty() {
        return format!(
            "actwatch — no tracked activity for {} ({})\n",
            range.label(),
            source_hint(range)
        );
    }

    let mut out = String::new();
    let total = report.total_secs();
    let mut summary = format!(
        "{} tracked across {}",
        human_duration(total),
        plural(report.distinct_apps(), "app")
    );
    summary.push_str(&away_phrase(report.away_secs()));
    let _ = writeln!(
        out,
        "actwatch — {} — {summary} · {}",
        range.label(),
        source_hint(range)
    );
    let _ = writeln!(out);

    for day in &report.days {
        let _ = writeln!(
            out,
            "{}  {} tracked · {}{} · {:.0}% of the range",
            day.day,
            human_duration(day.secs),
            plural(day.app_count(), "app"),
            if day.has_away() {
                format!(" · {} away", human_duration(day.away_secs))
            } else {
                String::new()
            },
            day.share_pct(report.tracked_secs()),
        );

        let shown: Vec<&UsageRow> = if limit == 0 {
            day.apps.iter().collect()
        } else {
            day.apps.iter().take(limit).collect()
        };

        let width = shown
            .iter()
            .map(|a| catalog.lookup(&a.app_id).name.chars().count())
            .chain(std::iter::once(crate::core_api::AWAY_LABEL.chars().count()))
            .max()
            .unwrap_or(4)
            .clamp(4, NAME_WIDTH_MAX);

        let _ = writeln!(
            out,
            "  {:<width$}  {:<cat$}  {:>9} {:>6}  {:>6} {:>8} {:>6}  share of app time",
            "app",
            "category",
            "time",
            "share",
            "cpu",
            "peak rss",
            "gpu",
            width = width,
            cat = CATEGORY_WIDTH,
        );

        for app in shown {
            let info = catalog.lookup(&app.app_id);
            let share = if total > 0 {
                (app.secs as f64 / total as f64) * 100.0
            } else {
                0.0
            };
            let _ = writeln!(
                out,
                "  {:<width$}  {:<cat$}  {:>9} {:>5.1}%  {:>6} {:>8} {:>6}  {}",
                info.name,
                info.category_label().unwrap_or("-"),
                human_duration(app.secs),
                share,
                human_pct(app.avg_cpu),
                human_rss(app.peak_rss_kb),
                human_pct(app.avg_gpu),
                bar(share / 100.0, BAR_WIDTH),
                width = width,
                cat = CATEGORY_WIDTH,
            );
        }

        if day.has_away() {
            // Idle time is real time, but not app time: it never appears in
            // the totals above and never gets a share of them.
            let _ = writeln!(
                out,
                "  {:<width$}  {:<cat$}  {:>9} {:>6}  {:>6} {:>8} {:>6}  {}",
                crate::core_api::AWAY_LABEL,
                "-",
                human_duration(day.away_secs),
                "-",
                "-",
                "-",
                "-",
                AWAY_NOTE,
                width = width,
                cat = CATEGORY_WIDTH,
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

/// ` · 23m away` when the range has idle time, empty otherwise.
pub fn away_phrase(secs: i64) -> String {
    if secs > 0 {
        format!(" · {} away", human_duration(secs))
    } else {
        String::new()
    }
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
///
/// The original column order is untouched, so existing consumers keep
/// working; `name`, `category` and `is_away` are appended. Idle time is one
/// extra row per day (`app_id = away`) with an empty `share_pct`, because
/// shares are computed against app time only.
pub fn render_csv(report: &RangeReport, catalog: &Catalog) -> String {
    let mut out = String::from(
        "day,app_id,secs,hours,share_pct,avg_cpu_pct,peak_rss_kb,avg_gpu_pct,source,name,category,is_away\n",
    );
    let total = report.total_secs();
    for day in &report.days {
        for app in &day.apps {
            let info = catalog.lookup(&app.app_id);
            let share = if total > 0 {
                (app.secs as f64 / total as f64) * 100.0
            } else {
                0.0
            };
            let _ = writeln!(
                out,
                "{},{},{},{:.4},{:.2},{},{},{},{},{},{},false",
                csv_escape(&app.day),
                csv_escape(&app.app_id),
                app.secs,
                app.secs as f64 / 3600.0,
                share,
                csv_num(app.avg_cpu),
                app.peak_rss_kb.map(|v| v.to_string()).unwrap_or_default(),
                csv_num(app.avg_gpu),
                app.source.as_str(),
                csv_escape(&info.name),
                csv_escape(info.category_label().unwrap_or("")),
            );
        }
        if day.has_away() {
            // day, app_id, secs, hours, then four empty fields (share and the
            // resource columns), then source, name, category, is_away.
            let _ = writeln!(
                out,
                "{},{},{},{:.4},,,,,{},{},,true",
                csv_escape(&day.day),
                crate::core_api::AWAY_APP_ID,
                day.away_secs,
                day.away_secs as f64 / 3600.0,
                day.apps.first().map(|a| a.source.as_str()).unwrap_or("auto"),
                crate::core_api::AWAY_LABEL,
            );
        }
    }
    out.trim_end().to_string()
}

/// Stable machine-readable payload for panels, scripts and the caelestia hook.
///
/// `totals.total_secs` keeps its meaning (app time, `away` excluded) so
/// existing consumers do not change; idle time is added as `away_secs`.
pub fn render_json(
    report: &RangeReport,
    range: &DateRange,
    catalog: &Catalog,
) -> Result<String, serde_json::Error> {
    let total = report.total_secs();
    // Day shares are relative to everything recorded (app time + idle), so the
    // days add up to 100%. Row shares stay relative to app time alone.
    let tracked = report.tracked_secs();
    let mut rows = Vec::new();
    let mut days = Vec::new();

    for day in &report.days {
        days.push(JsonDay {
            day: day.day.clone(),
            secs: day.secs,
            hours: day.secs as f64 / 3600.0,
            share_pct: day.share_pct(tracked),
            apps: day.app_count(),
            away_secs: day.away_secs,
        });
        for app in &day.apps {
            let info = catalog.lookup(&app.app_id);
            let category = info.category_label();
            rows.push(JsonUsageRow {
                day: app.day.clone(),
                app_id: app.app_id.clone(),
                name: info.name,
                category,
                secs: app.secs,
                hours: app.secs as f64 / 3600.0,
                share_pct: Some(if total > 0 {
                    (app.secs as f64 / total as f64) * 100.0
                } else {
                    0.0
                }),
                away: false,
                source: app.source.as_str(),
                avg_cpu_pct: app.avg_cpu,
                peak_rss_kb: app.peak_rss_kb,
                avg_gpu_pct: app.avg_gpu,
            });
        }
        if day.has_away() {
            rows.push(JsonUsageRow {
                day: day.day.clone(),
                app_id: crate::core_api::AWAY_APP_ID.to_string(),
                name: crate::core_api::AWAY_LABEL.to_string(),
                category: None,
                secs: day.away_secs,
                hours: day.away_secs as f64 / 3600.0,
                share_pct: None,
                away: true,
                source: day.apps.first().map(|a| a.source.as_str()).unwrap_or("auto"),
                avg_cpu_pct: None,
                peak_rss_kb: None,
                avg_gpu_pct: None,
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
            away_secs: report.away_secs(),
            away_hours: report.away_secs() as f64 / 3600.0,
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
    section(RULE, text)
}

pub fn warning(text: &str) -> String {
    format!("warning: {text}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{DayTotal, UsageRow};

    fn catalog() -> Catalog {
        Catalog::from_toml_str("[aliases]\nt3code = \"T3 Code\"\n")
    }

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
    fn table_uses_friendly_names_and_categories() {
        let r = report(vec![
            row("2026-10-10", "firefox", 3600),
            row("2026-10-10", "code", 1800),
            row("2026-10-10", "t3code", 600),
        ]);
        let out = render_table(&r, &range(), 0, &catalog());
        assert!(out.contains("Firefox"), "{out}");
        assert!(out.contains("Code"), "{out}");
        // Alias from the config file, not a prettified app id.
        assert!(out.contains("T3 Code"), "{out}");
        assert!(!out.contains("t3code"), "{out}");
        assert!(out.contains("Social"));
        assert!(out.contains("Productivity & Finance"));
    }

    #[test]
    fn away_is_its_own_row_and_excluded_from_totals() {
        let r = report(vec![
            row("2026-10-10", "firefox", 3600),
            row("2026-10-10", "away", 1800),
        ]);
        let out = render_table(&r, &range(), 0, &catalog());
        // Headline counts app time only.
        assert!(out.contains("1h tracked across 1 app · 30m away"), "{out}");
        // Away gets a row, flagged, with no share of app time.
        assert!(out.contains("Away"), "{out}");
        assert!(out.contains(AWAY_NOTE), "{out}");
        assert!(!out.contains("Other"), "{out}");
    }

    #[test]
    fn no_ansi_escapes_anywhere() {
        let r = report(vec![
            row("2026-10-10", "firefox", 3600),
            row("2026-10-10", "away", 1800),
        ]);
        let cat = catalog();
        let table = render_table(&r, &range(), 0, &cat);
        let csv = render_csv(&r, &cat);
        let json = render_json(&r, &range(), &cat).unwrap();
        let pairs = render_pairs(&[("daemon", "active".into())]);
        let head = heading("actwatch status");
        for (label, text) in [
            ("table", table),
            ("csv", csv),
            ("json", json),
            ("pairs", pairs),
            ("heading", head),
        ] {
            assert!(!text.contains('\u{1b}'), "{label} contains ANSI: {text}");
            assert!(!text.contains('\u{9b}'), "{label} contains CSI");
        }
    }

    #[test]
    fn csv_quotes_hostile_app_names() {
        let r = report(vec![row("2026-10-10", "my,app", 60)]);
        let csv = render_csv(&r, &catalog());
        assert!(csv.contains("\"my,app\""));
        assert!(csv.starts_with("day,app_id,secs"));
    }

    #[test]
    fn csv_appends_name_category_and_away_row() {
        let r = report(vec![
            row("2026-10-10", "mpv", 600),
            row("2026-10-10", "away", 300),
        ]);
        let csv = render_csv(&r, &catalog());
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines.len(), 3, "{csv}");
        assert!(lines[0].ends_with(",name,category,is_away"));
        assert!(lines[1].contains(",Mpv,Media,false"));
        assert!(lines[2].contains(",away,"));
        assert!(lines[2].ends_with(",Away,,true"), "{csv}");
        // Away has no share: shares are relative to app time.
        assert!(lines[2].starts_with("2026-10-10,away,300,0.0833,,,,,rollup"), "{csv}");
    }

    #[test]
    fn json_roundtrips_totals_and_names() {
        let r = report(vec![
            row("2026-10-10", "firefox", 3600),
            row("2026-10-10", "away", 900),
        ]);
        let json = render_json(&r, &range(), &catalog()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        // totals.total_secs keeps meaning app time only.
        assert_eq!(v["totals"]["total_secs"], 3600);
        assert_eq!(v["totals"]["away_secs"], 900);
        assert_eq!(v["totals"]["apps"], 1);
        assert_eq!(v["days"][0]["away_secs"], 900);
        // Day share covers idle too: 4500 of 4500 tracked seconds.
        assert!((v["days"][0]["share_pct"].as_f64().unwrap() - 100.0).abs() < 0.01);
        assert_eq!(v["rows"][0]["app_id"], "firefox");
        assert_eq!(v["rows"][0]["name"], "Firefox");
        assert_eq!(v["rows"][0]["category"], "Social");
        assert_eq!(v["rows"][0]["away"], false);
        // Away is its own row, with no category and no share.
        assert_eq!(v["rows"][1]["name"], "Away");
        assert_eq!(v["rows"][1]["away"], true);
        assert_eq!(v["rows"][1]["category"], serde_json::Value::Null);
        assert_eq!(v["rows"][1]["share_pct"], serde_json::Value::Null);
    }

    #[test]
    fn json_never_leaks_window_titles() {
        // The schema stores a title column; the CLI is app-level only.
        let r = report(vec![row("2026-10-10", "firefox", 60)]);
        let json = render_json(&r, &range(), &catalog()).unwrap();
        assert!(!json.contains("title"));
        assert!(!render_csv(&r, &catalog()).contains("title"));
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
        assert!(render_table(&r, &range(), 0, &catalog()).contains("no tracked activity"));
        assert!(render_csv(&r, &catalog()).starts_with("day,"));
    }

    #[test]
    fn limit_truncates_with_notice() {
        let r = report(vec![
            row("2026-10-10", "a", 300),
            row("2026-10-10", "b", 200),
        ]);
        let out = render_table(&r, &range(), 1, &catalog());
        assert!(out.contains("1 more app"));
        let full = render_table(&r, &range(), 0, &catalog());
        assert!(!full.contains("more app"));
    }

    #[test]
    fn away_row_is_dropped_when_filtered_away() {
        let mut day = DayTotal {
            day: "2026-10-10".into(),
            secs: 60,
            away_secs: 0,
            apps: vec![row("2026-10-10", "firefox", 60)],
        };
        day.away_secs = 0;
        assert!(!day.has_away());
        assert_eq!(away_phrase(0), "");
        assert_eq!(away_phrase(1800), " · 30m away");
    }

    #[test]
    fn day_totals_share_percentage() {
        let d = DayTotal {
            day: "2026-10-10".into(),
            secs: 3600,
            away_secs: 0,
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