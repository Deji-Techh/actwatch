use anyhow::Result;
use chrono::{Datelike, Duration, NaiveDate};
use ratatui::style::Color;
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Category {
    Social,
    Productivity,
    Utilities,
    Other,
}

impl Category {
    pub const ALL: [Category; 4] = [
        Category::Social,
        Category::Productivity,
        Category::Utilities,
        Category::Other,
    ];

    pub fn name(&self) -> &'static str {
        match self {
            Category::Social => "Social",
            Category::Productivity => "Productivity & Finance",
            Category::Utilities => "Utilities",
            Category::Other => "Other",
        }
    }

    pub fn short_name(&self) -> &'static str {
        match self {
            Category::Social => "Social",
            Category::Productivity => "Productivity",
            Category::Utilities => "Utilities",
            Category::Other => "Other",
        }
    }

    pub fn color(&self) -> Color {
        match self {
            Category::Social => Color::Rgb(56, 189, 248),       // Sky / Cyan
            Category::Productivity => Color::Rgb(52, 211, 153), // Emerald / Green
            Category::Utilities => Color::Rgb(251, 146, 60),    // Amber / Orange
            Category::Other => Color::Rgb(156, 163, 175),       // Slate / Gray
        }
    }
}

pub fn classify_app(app_id: &str) -> Category {
    let lower = app_id.to_lowercase();

    if lower.contains("discord")
        || lower.contains("telegram")
        || lower.contains("slack")
        || lower.contains("element")
        || lower.contains("signal")
        || lower.contains("matrix")
        || lower.contains("twitter")
        || lower.contains("reddit")
        || lower.contains("bsky")
        || lower.contains("mastodon")
        || lower.contains("whatsapp")
        || lower.contains("ferdium")
        || lower.contains("webcord")
        || lower.contains("vesktop")
        || lower.contains("zoom")
        || lower.contains("teams")
        || lower.contains("messenger")
        || lower.contains("skype")
    {
        return Category::Social;
    }

    if lower.contains("code")
        || lower.contains("vscodium")
        || lower.contains("nvim")
        || lower.contains("vim")
        || lower.contains("emacs")
        || lower.contains("alacritty")
        || lower.contains("kitty")
        || lower.contains("foot")
        || lower.contains("wezterm")
        || lower.contains("ghostty")
        || lower.contains("terminal")
        || lower.contains("libreoffice")
        || lower.contains("calc")
        || lower.contains("writer")
        || lower.contains("impress")
        || lower.contains("obsidian")
        || lower.contains("notion")
        || lower.contains("cargo")
        || lower.contains("rustc")
        || lower.contains("git")
        || lower.contains("intellij")
        || lower.contains("clion")
        || lower.contains("pycharm")
        || lower.contains("gimp")
        || lower.contains("blender")
        || lower.contains("inkscape")
        || lower.contains("thunderbird")
        || lower.contains("firefox")
        || lower.contains("chromium")
        || lower.contains("chrome")
        || lower.contains("brave")
        || lower.contains("zen")
        || lower.contains("postman")
        || lower.contains("dbeaver")
    {
        return Category::Productivity;
    }

    if lower.contains("hyprland")
        || lower.contains("waybar")
        || lower.contains("rofi")
        || lower.contains("wofi")
        || lower.contains("nemo")
        || lower.contains("thunar")
        || lower.contains("dolphin")
        || lower.contains("pavucontrol")
        || lower.contains("btop")
        || lower.contains("htop")
        || lower.contains("settings")
        || lower.contains("systemsettings")
        || lower.contains("actwatch")
        || lower.contains("dunst")
        || lower.contains("mako")
        || lower.contains("wlogout")
        || lower.contains("filemanager")
        || lower.contains("calculator")
    {
        return Category::Utilities;
    }

    Category::Other
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct AppUsageRow {
    pub app_id: String,
    pub category: Category,
    pub duration_secs: u64,
    pub avg_cpu: f64,
    pub peak_rss_kb: u64,
    pub avg_gpu: f64,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct DaySummary {
    pub date: NaiveDate,
    pub day_name: &'static str, // "Sun", "Mon", ...
    pub total_secs: u64,
    pub category_secs: HashMap<Category, u64>,
    pub is_selected: bool,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct WeeklyChartData {
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub days: Vec<DaySummary>,
    pub avg_daily_secs: u64,
    pub max_daily_secs: u64,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct CategoryTotal {
    pub category: Category,
    pub total_secs: u64,
    pub percentage: f64,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct DashboardData {
    pub selected_date: NaiveDate,
    pub total_usage_secs: u64,
    pub wow_delta_pct: Option<f64>,
    pub wow_prev_secs: u64,
    pub chart: WeeklyChartData,
    pub category_breakdown: Vec<CategoryTotal>,
    pub apps: Vec<AppUsageRow>,
    pub is_empty_db: bool,
    pub db_path: PathBuf,
}

pub struct Database {
    path: PathBuf,
}

impl Database {
    pub fn default_path() -> PathBuf {
        if let Ok(path_str) = std::env::var("ACTWATCH_DB") {
            return PathBuf::from(path_str);
        }

        if let Ok(home) = std::env::var("HOME") {
            PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("actwatch")
                .join("activity.db")
        } else {
            PathBuf::from("activity.db")
        }
    }

    pub fn new(path: Option<PathBuf>) -> Self {
        Self {
            path: path.unwrap_or_else(Self::default_path),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn open_connection(&self) -> Result<Option<Connection>> {
        if !self.path.exists() {
            return Ok(None);
        }

        let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        match Connection::open_with_flags(&self.path, flags) {
            Ok(conn) => Ok(Some(conn)),
            Err(_) => Ok(None),
        }
    }

    pub fn fetch_dashboard_data(&self, selected_date: NaiveDate) -> Result<DashboardData> {
        let empty_chart = self.generate_empty_week_chart(selected_date);
        let default_categories: Vec<CategoryTotal> = Category::ALL
            .iter()
            .map(|&c| CategoryTotal {
                category: c,
                total_secs: 0,
                percentage: 0.0,
            })
            .collect();

        let conn_opt = self.open_connection()?;

        let Some(conn) = conn_opt else {
            return Ok(DashboardData {
                selected_date,
                total_usage_secs: 0,
                wow_delta_pct: None,
                wow_prev_secs: 0,
                chart: empty_chart,
                category_breakdown: default_categories,
                apps: Vec::new(),
                is_empty_db: true,
                db_path: self.path.clone(),
            });
        };

        if !self.has_tables_or_data(&conn) {
            return Ok(DashboardData {
                selected_date,
                total_usage_secs: 0,
                wow_delta_pct: None,
                wow_prev_secs: 0,
                chart: empty_chart,
                category_breakdown: default_categories,
                apps: Vec::new(),
                is_empty_db: true,
                db_path: self.path.clone(),
            });
        }

        // Fetch selected day apps
        let apps = self.fetch_apps_for_day(&conn, selected_date)?;
        let total_usage_secs: u64 = apps.iter().map(|a| a.duration_secs).sum();

        // Fetch category breakdown for selected day
        let mut cat_map: HashMap<Category, u64> = HashMap::new();
        for app in &apps {
            *cat_map.entry(app.category).or_insert(0) += app.duration_secs;
        }

        let mut category_breakdown = Vec::new();
        for cat in Category::ALL {
            let secs = *cat_map.get(&cat).unwrap_or(&0);
            let percentage = if total_usage_secs > 0 {
                (secs as f64 / total_usage_secs as f64) * 100.0
            } else {
                0.0
            };
            category_breakdown.push(CategoryTotal {
                category: cat,
                total_secs: secs,
                percentage,
            });
        }

        // Fetch Week-over-Week previous week day total (same weekday 7 days ago)
        let prev_week_date = selected_date - Duration::days(7);
        let prev_week_apps = self.fetch_apps_for_day(&conn, prev_week_date)?;
        let wow_prev_secs: u64 = prev_week_apps.iter().map(|a| a.duration_secs).sum();

        let wow_delta_pct = if wow_prev_secs > 0 {
            Some(((total_usage_secs as f64 - wow_prev_secs as f64) / wow_prev_secs as f64) * 100.0)
        } else if total_usage_secs > 0 {
            Some(100.0)
        } else {
            None
        };

        // Fetch 7-day chart (Sunday to Saturday of the selected week)
        let chart = self.fetch_weekly_chart(&conn, selected_date)?;

        let has_any_data = total_usage_secs > 0 || chart.days.iter().any(|d| d.total_secs > 0);

        Ok(DashboardData {
            selected_date,
            total_usage_secs,
            wow_delta_pct,
            wow_prev_secs,
            chart,
            category_breakdown,
            apps,
            is_empty_db: !has_any_data && self.is_db_completely_empty(&conn),
            db_path: self.path.clone(),
        })
    }

    fn has_tables_or_data(&self, conn: &Connection) -> bool {
        let count: Result<i64, _> = conn.query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name IN ('daily_rollup', 'app_events', 'resource_samples')",
            [],
            |r| r.get(0),
        );
        match count {
            Ok(c) => c > 0,
            Err(_) => false,
        }
    }

    fn is_db_completely_empty(&self, conn: &Connection) -> bool {
        let rollup_count: i64 = conn
            .query_row("SELECT count(*) FROM daily_rollup", [], |r| r.get(0))
            .unwrap_or(0);
        let events_count: i64 = conn
            .query_row("SELECT count(*) FROM app_events", [], |r| r.get(0))
            .unwrap_or(0);
        rollup_count == 0 && events_count == 0
    }

    fn generate_empty_week_chart(&self, selected_date: NaiveDate) -> WeeklyChartData {
        let weekday = selected_date.weekday();
        let days_from_sun = weekday.num_days_from_sunday();
        let start_sun = selected_date - Duration::days(days_from_sun as i64);

        let day_names = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
        let mut days = Vec::with_capacity(7);

        for i in 0..7 {
            let current_date = start_sun + Duration::days(i);
            days.push(DaySummary {
                date: current_date,
                day_name: day_names[i as usize],
                total_secs: 0,
                category_secs: HashMap::new(),
                is_selected: current_date == selected_date,
            });
        }

        WeeklyChartData {
            start_date: start_sun,
            end_date: start_sun + Duration::days(6),
            days,
            avg_daily_secs: 0,
            max_daily_secs: 0,
        }
    }

    fn fetch_apps_for_day(&self, conn: &Connection, date: NaiveDate) -> Result<Vec<AppUsageRow>> {
        let date_str = date.format("%Y-%m-%d").to_string();

        // 1. Try reading from daily_rollup
        let has_rollup_table: bool = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='daily_rollup'",
                [],
                |r| r.get::<_, i64>(0),
            )
            .map(|c| c > 0)
            .unwrap_or(false);

        if has_rollup_table {
            let mut stmt = conn.prepare(
                "SELECT app_id, secs, avg_cpu, peak_rss_kb, avg_gpu FROM daily_rollup WHERE day = ?1 ORDER BY secs DESC",
            )?;

            let rows = stmt.query_map([&date_str], |row| {
                let app_id: String = row.get(0)?;
                let secs: i64 = row.get(1)?;
                let avg_cpu: f64 = row.get(2).unwrap_or(0.0);
                let peak_rss_kb: i64 = row.get(3).unwrap_or(0);
                let avg_gpu: f64 = row.get(4).unwrap_or(0.0);

                let category = classify_app(&app_id);

                Ok(AppUsageRow {
                    app_id,
                    category,
                    duration_secs: secs.max(0) as u64,
                    avg_cpu,
                    peak_rss_kb: peak_rss_kb.max(0) as u64,
                    avg_gpu,
                })
            })?;

            let mut app_list = Vec::new();
            for r in rows {
                if let Ok(app) = r {
                    app_list.push(app);
                }
            }

            if !app_list.is_empty() {
                return Ok(app_list);
            }
        }

        // 2. Fallback: Aggregate from app_events and resource_samples
        let has_events_table: bool = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='app_events'",
                [],
                |r| r.get::<_, i64>(0),
            )
            .map(|c| c > 0)
            .unwrap_or(false);

        if !has_events_table {
            return Ok(Vec::new());
        }

        // Query app_events for duration
        let mut stmt = conn.prepare(
            "SELECT app_id, SUM(dur_secs) FROM app_events 
             WHERE (date(ts, 'unixepoch', 'localtime') = ?1 OR date(ts, 'unixepoch') = ?1)
             GROUP BY app_id 
             ORDER BY SUM(dur_secs) DESC",
        )?;

        let event_rows = stmt.query_map([&date_str], |row| {
            let app_id: String = row.get(0)?;
            let total_dur: i64 = row.get(1).unwrap_or(0);
            Ok((app_id, total_dur.max(0) as u64))
        })?;

        let mut app_durations: HashMap<String, u64> = HashMap::new();
        for r in event_rows {
            if let Ok((app, dur)) = r {
                app_durations.insert(app, dur);
            }
        }

        if app_durations.is_empty() {
            return Ok(Vec::new());
        }

        // Query resource_samples for metrics
        let mut res_map: HashMap<String, (f64, u64, f64)> = HashMap::new();
        let has_resources_table: bool = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='resource_samples'",
                [],
                |r| r.get::<_, i64>(0),
            )
            .map(|c| c > 0)
            .unwrap_or(false);

        if has_resources_table {
            if let Ok(mut stmt_res) = conn.prepare(
                "SELECT app_id, AVG(cpu_pct), MAX(rss_kb), AVG(gpu_pct) FROM resource_samples
                 WHERE (date(ts, 'unixepoch', 'localtime') = ?1 OR date(ts, 'unixepoch') = ?1)
                 GROUP BY app_id",
            ) {
                let res_rows = stmt_res.query_map([&date_str], |row| {
                    let app_id: String = row.get(0)?;
                    let avg_cpu: f64 = row.get(1).unwrap_or(0.0);
                    let max_rss: i64 = row.get(2).unwrap_or(0);
                    let avg_gpu: f64 = row.get(3).unwrap_or(0.0);
                    Ok((app_id, (avg_cpu, max_rss.max(0) as u64, avg_gpu)))
                });

                if let Ok(iter) = res_rows {
                    for r in iter {
                        if let Ok((app, metrics)) = r {
                            res_map.insert(app, metrics);
                        }
                    }
                }
            }
        }

        let mut app_list = Vec::new();
        for (app_id, duration_secs) in app_durations {
            let category = classify_app(&app_id);
            let (avg_cpu, peak_rss_kb, avg_gpu) = res_map.get(&app_id).copied().unwrap_or((0.0, 0, 0.0));
            app_list.push(AppUsageRow {
                app_id,
                category,
                duration_secs,
                avg_cpu,
                peak_rss_kb,
                avg_gpu,
            });
        }

        app_list.sort_by(|a, b| b.duration_secs.cmp(&a.duration_secs));

        Ok(app_list)
    }

    fn fetch_weekly_chart(&self, conn: &Connection, selected_date: NaiveDate) -> Result<WeeklyChartData> {
        let weekday = selected_date.weekday();
        let days_from_sun = weekday.num_days_from_sunday();
        let start_sun = selected_date - Duration::days(days_from_sun as i64);

        let day_names = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
        let mut days = Vec::with_capacity(7);
        let mut total_week_secs: u64 = 0;
        let mut max_daily_secs: u64 = 0;

        for i in 0..7 {
            let current_date = start_sun + Duration::days(i);
            let apps = self.fetch_apps_for_day(conn, current_date)?;

            let day_total: u64 = apps.iter().map(|a| a.duration_secs).sum();
            total_week_secs += day_total;
            if day_total > max_daily_secs {
                max_daily_secs = day_total;
            }

            let mut category_secs = HashMap::new();
            for app in &apps {
                *category_secs.entry(app.category).or_insert(0) += app.duration_secs;
            }

            days.push(DaySummary {
                date: current_date,
                day_name: day_names[i as usize],
                total_secs: day_total,
                category_secs,
                is_selected: current_date == selected_date,
            });
        }

        let avg_daily_secs = total_week_secs / 7;

        Ok(WeeklyChartData {
            start_date: start_sun,
            end_date: start_sun + Duration::days(6),
            days,
            avg_daily_secs,
            max_daily_secs,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_classify_app() {
        assert_eq!(classify_app("discord"), Category::Social);
        assert_eq!(classify_app("org.telegram.desktop"), Category::Social);
        assert_eq!(classify_app("Slack"), Category::Social);
        assert_eq!(classify_app("code"), Category::Productivity);
        assert_eq!(classify_app("nvim"), Category::Productivity);
        assert_eq!(classify_app("Alacritty"), Category::Productivity);
        assert_eq!(classify_app("firefox"), Category::Productivity);
        assert_eq!(classify_app("hyprland"), Category::Utilities);
        assert_eq!(classify_app("waybar"), Category::Utilities);
        assert_eq!(classify_app("pavucontrol"), Category::Utilities);
        assert_eq!(classify_app("unknown_custom_app"), Category::Other);
    }

    #[test]
    fn test_empty_db_detection() {
        let test_dir = std::env::temp_dir().join("actwatch_test_empty");
        let _ = fs::create_dir_all(&test_dir);
        let db_path = test_dir.join("nonexistent.db");
        let _ = fs::remove_file(&db_path);

        let db = Database::new(Some(db_path));
        let data = db.fetch_dashboard_data(NaiveDate::from_ymd_opt(2026, 10, 10).unwrap()).unwrap();
        assert!(data.is_empty_db);
        assert_eq!(data.total_usage_secs, 0);
        assert_eq!(data.apps.len(), 0);
    }

    #[test]
    fn test_fetch_from_daily_rollup() {
        let test_dir = std::env::temp_dir().join("actwatch_test_rollup");
        let _ = fs::create_dir_all(&test_dir);
        let db_path = test_dir.join("activity.db");
        let _ = fs::remove_file(&db_path);

        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "
            CREATE TABLE daily_rollup(day TEXT, app_id TEXT, secs INTEGER, avg_cpu REAL, peak_rss_kb INTEGER, avg_gpu REAL);
            CREATE TABLE app_events(ts INTEGER, app_id TEXT, title TEXT, dur_secs INTEGER);
            CREATE TABLE resource_samples(ts INTEGER, app_id TEXT, pid INTEGER, cpu_pct REAL, rss_kb INTEGER, gpu_pct REAL);
            CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT);
            ",
        ).unwrap();

        let date = NaiveDate::from_ymd_opt(2026, 10, 10).unwrap();
        let date_str = date.format("%Y-%m-%d").to_string();

        conn.execute(
            "INSERT INTO daily_rollup (day, app_id, secs, avg_cpu, peak_rss_kb, avg_gpu) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            (&date_str, "org.wezfurlong.wezterm", 14400, 2.5, 120000, 0.5),
        ).unwrap();

        conn.execute(
            "INSERT INTO daily_rollup (day, app_id, secs, avg_cpu, peak_rss_kb, avg_gpu) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            (&date_str, "discord", 7200, 4.1, 450000, 1.2),
        ).unwrap();

        let db = Database::new(Some(db_path.clone()));
        let data = db.fetch_dashboard_data(date).unwrap();

        assert!(!data.is_empty_db);
        assert_eq!(data.total_usage_secs, 14400 + 7200);
        assert_eq!(data.apps.len(), 2);
        assert_eq!(data.apps[0].app_id, "org.wezfurlong.wezterm");
        assert_eq!(data.apps[0].category, Category::Productivity);
        assert_eq!(data.apps[1].app_id, "discord");
        assert_eq!(data.apps[1].category, Category::Social);

        let _ = fs::remove_file(&db_path);
    }
}
