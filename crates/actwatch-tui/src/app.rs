use anyhow::Result;
use chrono::{Local, NaiveDate};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::db::{AppUsageRow, DashboardData, Database};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppMode {
    Normal,
    Search,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortMode {
    Time,
    Cpu,
    Memory,
    Gpu,
}

impl SortMode {
    pub fn label(&self) -> &'static str {
        match self {
            SortMode::Time => "Time",
            SortMode::Cpu => "Avg CPU",
            SortMode::Memory => "Peak RSS",
            SortMode::Gpu => "Avg GPU",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortOrder {
    Ascending,
    Descending,
}

pub struct App {
    pub db: Database,
    pub selected_date: NaiveDate,
    pub mode: AppMode,
    pub search_query: String,
    pub sort_mode: SortMode,
    pub sort_order: SortOrder,
    pub selected_app_index: usize,
    pub data: DashboardData,
    pub last_refresh: Instant,
    pub refresh_interval: Duration,
    pub should_quit: bool,
    pub status_message: Option<(String, Instant)>,
    pub error_message: Option<String>,
}

impl App {
    pub fn new(db_path: Option<PathBuf>, initial_date: Option<NaiveDate>) -> Result<Self> {
        let db = Database::new(db_path);
        let selected_date = initial_date.unwrap_or_else(|| Local::now().date_naive());

        let data = match db.fetch_dashboard_data(selected_date) {
            Ok(d) => d,
            Err(_e) => db.fetch_dashboard_data(selected_date).unwrap_or_else(|_| {
                DashboardData {
                    selected_date,
                    total_usage_secs: 0,
                    away_secs: 0,
                    wow_delta_pct: None,
                    chart: crate::db::WeeklyChartData {
                        start_date: selected_date,
                        end_date: selected_date,
                        days: Vec::new(),
                        avg_daily_secs: 0,
                        max_daily_secs: 0,
                    },
                    category_breakdown: Vec::new(),
                    apps: Vec::new(),
                    is_empty_db: true,
                    is_all_away: false,
                    db_path: db.path().to_path_buf(),
                }
            }),
        };

        Ok(Self {
            db,
            selected_date,
            mode: AppMode::Normal,
            search_query: String::new(),
            sort_mode: SortMode::Time,
            sort_order: SortOrder::Descending,
            selected_app_index: 0,
            data,
            last_refresh: Instant::now(),
            refresh_interval: Duration::from_secs(30),
            should_quit: false,
            status_message: None,
            error_message: None,
        })
    }

    pub fn refresh_data(&mut self) {
        match self.db.fetch_dashboard_data(self.selected_date) {
            Ok(data) => {
                self.data = data;
                self.error_message = None;
            }
            Err(e) => {
                self.error_message = Some(format!("Error reading DB: {}", e));
            }
        }
        self.last_refresh = Instant::now();
        self.clamp_selected_index();
    }

    pub fn tick(&mut self) {
        if self.last_refresh.elapsed() >= self.refresh_interval {
            self.refresh_data();
        }

        // Expire status message after 3 seconds
        if let Some((_, created_at)) = self.status_message {
            if created_at.elapsed() > Duration::from_secs(3) {
                self.status_message = None;
            }
        }
    }

    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status_message = Some((msg.into(), Instant::now()));
    }

    pub fn prev_day(&mut self) {
        self.selected_date = self.selected_date - chrono::Duration::days(1);
        self.refresh_data();
        self.set_status(format!("Navigated to {}", self.selected_date.format("%a, %b %d")));
    }

    pub fn next_day(&mut self) {
        self.selected_date = self.selected_date + chrono::Duration::days(1);
        self.refresh_data();
        self.set_status(format!("Navigated to {}", self.selected_date.format("%a, %b %d")));
    }

    pub fn go_to_today(&mut self) {
        let today = Local::now().date_naive();
        if self.selected_date != today {
            self.selected_date = today;
            self.refresh_data();
            self.set_status("Jumped to Today");
        }
    }

    pub fn prev_week(&mut self) {
        self.selected_date = self.selected_date - chrono::Duration::days(7);
        self.refresh_data();
        self.set_status(format!("Week shifted to {}", self.selected_date.format("%a, %b %d")));
    }

    pub fn next_week(&mut self) {
        self.selected_date = self.selected_date + chrono::Duration::days(7);
        self.refresh_data();
        self.set_status(format!("Week shifted to {}", self.selected_date.format("%a, %b %d")));
    }

    pub fn set_sort_mode(&mut self, mode: SortMode) {
        if self.sort_mode == mode {
            self.sort_order = match self.sort_order {
                SortOrder::Descending => SortOrder::Ascending,
                SortOrder::Ascending => SortOrder::Descending,
            };
        } else {
            self.sort_mode = mode;
            self.sort_order = SortOrder::Descending;
        }
        let dir_str = match self.sort_order {
            SortOrder::Descending => "▼ desc",
            SortOrder::Ascending => "▲ asc",
        };
        self.set_status(format!("Sort: {} {}", self.sort_mode.label(), dir_str));
        self.clamp_selected_index();
    }

    pub fn cycle_sort_mode(&mut self) {
        let next_mode = match self.sort_mode {
            SortMode::Time => SortMode::Cpu,
            SortMode::Cpu => SortMode::Memory,
            SortMode::Memory => SortMode::Gpu,
            SortMode::Gpu => SortMode::Time,
        };
        self.set_sort_mode(next_mode);
    }

    pub fn get_filtered_and_sorted_apps(&self) -> Vec<&AppUsageRow> {
        let query = self.search_query.trim().to_lowercase();

        let mut filtered: Vec<&AppUsageRow> = self
            .data
            .apps
            .iter()
            .filter(|app| {
                if query.is_empty() {
                    return true;
                }
                app.app_id.to_lowercase().contains(&query)
                    || app.display_name.to_lowercase().contains(&query)
                    || app.category.name().to_lowercase().contains(&query)
                    || app.category.short_name().to_lowercase().contains(&query)
            })
            .collect();

        filtered.sort_by(|a, b| {
            let ordering = match self.sort_mode {
                SortMode::Time => a.duration_secs.cmp(&b.duration_secs),
                SortMode::Cpu => a
                    .avg_cpu
                    .partial_cmp(&b.avg_cpu)
                    .unwrap_or(std::cmp::Ordering::Equal),
                SortMode::Memory => a.peak_rss_kb.cmp(&b.peak_rss_kb),
                SortMode::Gpu => a
                    .avg_gpu
                    .partial_cmp(&b.avg_gpu)
                    .unwrap_or(std::cmp::Ordering::Equal),
            };

            match self.sort_order {
                SortOrder::Descending => ordering.reverse(),
                SortOrder::Ascending => ordering,
            }
        });

        filtered
    }

    pub fn clamp_selected_index(&mut self) {
        let count = self.get_filtered_and_sorted_apps().len();
        if count == 0 {
            self.selected_app_index = 0;
        } else if self.selected_app_index >= count {
            self.selected_app_index = count - 1;
        }
    }

    pub fn select_next(&mut self) {
        let count = self.get_filtered_and_sorted_apps().len();
        if count > 0 {
            if self.selected_app_index + 1 < count {
                self.selected_app_index += 1;
            } else {
                self.selected_app_index = 0;
            }
        }
    }

    pub fn select_prev(&mut self) {
        let count = self.get_filtered_and_sorted_apps().len();
        if count > 0 {
            if self.selected_app_index > 0 {
                self.selected_app_index -= 1;
            } else {
                self.selected_app_index = count - 1;
            }
        }
    }

    pub fn select_page_down(&mut self, page_size: usize) {
        let count = self.get_filtered_and_sorted_apps().len();
        if count > 0 {
            self.selected_app_index = (self.selected_app_index + page_size).min(count - 1);
        }
    }

    pub fn select_page_up(&mut self, page_size: usize) {
        if self.selected_app_index >= page_size {
            self.selected_app_index -= page_size;
        } else {
            self.selected_app_index = 0;
        }
    }

    pub fn select_first(&mut self) {
        self.selected_app_index = 0;
    }

    pub fn select_last(&mut self) {
        let count = self.get_filtered_and_sorted_apps().len();
        if count > 0 {
            self.selected_app_index = count - 1;
        }
    }

    pub fn on_key(&mut self, key: KeyEvent) {
        match self.mode {
            AppMode::Search => match key.code {
                KeyCode::Esc => {
                    self.mode = AppMode::Normal;
                    if self.search_query.is_empty() {
                        self.set_status("Search closed");
                    } else {
                        self.set_status(format!("Filter active: \"{}\"", self.search_query));
                    }
                }
                KeyCode::Enter => {
                    self.mode = AppMode::Normal;
                    self.set_status(format!("Filter active: \"{}\"", self.search_query));
                }
                KeyCode::Backspace => {
                    self.search_query.pop();
                    self.clamp_selected_index();
                }
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.search_query.clear();
                    self.mode = AppMode::Normal;
                }
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.search_query.clear();
                    self.clamp_selected_index();
                }
                KeyCode::Char(c) => {
                    self.search_query.push(c);
                    self.clamp_selected_index();
                }
                _ => {}
            },
            AppMode::Normal => match key.code {
                KeyCode::Char('q') => self.should_quit = true,
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.should_quit = true
                }
                KeyCode::Char('/') => {
                    self.mode = AppMode::Search;
                    self.set_status("Search mode active (Esc to exit, Enter to keep)");
                }
                KeyCode::Esc => {
                    if !self.search_query.is_empty() {
                        self.search_query.clear();
                        self.clamp_selected_index();
                        self.set_status("Filter cleared");
                    }
                }
                KeyCode::Char('1') => self.set_sort_mode(SortMode::Time),
                KeyCode::Char('2') => self.set_sort_mode(SortMode::Cpu),
                KeyCode::Char('3') => self.set_sort_mode(SortMode::Memory),
                KeyCode::Char('4') => self.set_sort_mode(SortMode::Gpu),
                KeyCode::Char('s') => self.cycle_sort_mode(),

                // Day navigation
                KeyCode::Left | KeyCode::Char('h') => self.prev_day(),
                KeyCode::Right | KeyCode::Char('l') => self.next_day(),
                KeyCode::Char('t') => self.go_to_today(),

                // Week navigation
                KeyCode::Char('[') => self.prev_week(),
                KeyCode::Char(']') => self.next_week(),

                // Reload
                KeyCode::Char('r') => {
                    self.refresh_data();
                    self.set_status("Reloaded from database");
                }

                // Table scrolling
                KeyCode::Up | KeyCode::Char('k') => self.select_prev(),
                KeyCode::Down | KeyCode::Char('j') => self.select_next(),
                KeyCode::PageUp => self.select_page_up(10),
                KeyCode::PageDown => self.select_page_down(10),
                KeyCode::Home | KeyCode::Char('g') => self.select_first(),
                KeyCode::End | KeyCode::Char('G') => self.select_last(),

                _ => {}
            },
        }
    }
}

pub fn format_duration(secs: u64) -> String {
    let hours = secs / 3600;
    let mins = (secs % 3600) / 60;
    let rem_secs = secs % 60;

    if hours > 0 {
        format!("{}h {:02}m", hours, mins)
    } else if mins > 0 {
        format!("{}m {:02}s", mins, rem_secs)
    } else {
        format!("{}s", rem_secs)
    }
}

pub fn format_rss(kb: u64) -> String {
    if kb >= 1024 * 1024 {
        format!("{:.1} GB", kb as f64 / (1024.0 * 1024.0))
    } else if kb >= 1024 {
        format!("{:.1} MB", kb as f64 / 1024.0)
    } else if kb > 0 {
        format!("{} KB", kb)
    } else {
        "-".to_string()
    }
}

pub fn format_pct(pct: f64) -> String {
    if pct > 0.05 {
        format!("{:.1}%", pct)
    } else {
        "-".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Category;

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(55260), "15h 21m");
        assert_eq!(format_duration(3600), "1h 00m");
        assert_eq!(format_duration(125), "2m 05s");
        assert_eq!(format_duration(42), "42s");
    }

    #[test]
    fn test_format_rss() {
        assert_eq!(format_rss(1024 * 1024 * 2), "2.0 GB");
        assert_eq!(format_rss(512 * 1024), "512.0 MB");
        assert_eq!(format_rss(500), "500 KB");
        assert_eq!(format_rss(0), "-");
    }

    #[test]
    fn test_app_sorting_and_filtering() {
        let mut app = App::new(None, Some(NaiveDate::from_ymd_opt(2026, 10, 10).unwrap())).unwrap();

        app.data.apps = vec![
            AppUsageRow {
                app_id: "discord".to_string(),
                display_name: "Discord".to_string(),
                category: Category::Social,
                duration_secs: 3600,
                avg_cpu: 2.0,
                peak_rss_kb: 500000,
                avg_gpu: 1.0,
            },
            AppUsageRow {
                app_id: "com.t3tools.T3Code".to_string(),
                display_name: "T3Code".to_string(),
                category: Category::ProductivityFinance,
                duration_secs: 7200,
                avg_cpu: 8.5,
                peak_rss_kb: 1200000,
                avg_gpu: 3.0,
            },
            AppUsageRow {
                app_id: "htop".to_string(),
                display_name: "Htop".to_string(),
                category: Category::Utilities,
                duration_secs: 300,
                avg_cpu: 0.5,
                peak_rss_kb: 15000,
                avg_gpu: 0.0,
            },
        ];

        // Default sort: Time Descending -> T3Code, Discord, Htop
        let sorted = app.get_filtered_and_sorted_apps();
        assert_eq!(sorted.len(), 3);
        assert_eq!(sorted[0].app_id, "com.t3tools.T3Code");
        assert_eq!(sorted[1].app_id, "discord");
        assert_eq!(sorted[2].app_id, "htop");

        // Sort by CPU Descending -> T3Code (8.5%), Discord (2.0%), Htop (0.5%)
        app.set_sort_mode(SortMode::Cpu);
        let sorted_cpu = app.get_filtered_and_sorted_apps();
        assert_eq!(sorted_cpu[0].app_id, "com.t3tools.T3Code");
        assert_eq!(sorted_cpu[1].app_id, "discord");
        assert_eq!(sorted_cpu[2].app_id, "htop");

        // Toggle CPU to Ascending
        app.set_sort_mode(SortMode::Cpu);
        let sorted_cpu_asc = app.get_filtered_and_sorted_apps();
        assert_eq!(sorted_cpu_asc[0].app_id, "htop");
        assert_eq!(sorted_cpu_asc[1].app_id, "discord");
        assert_eq!(sorted_cpu_asc[2].app_id, "com.t3tools.T3Code");

        // Search filter by display name
        app.search_query = "T3".to_string();
        let filtered = app.get_filtered_and_sorted_apps();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].display_name, "T3Code");
    }

    #[test]
    fn test_day_navigation() {
        let initial_date = NaiveDate::from_ymd_opt(2026, 10, 10).unwrap();
        let mut app = App::new(None, Some(initial_date)).unwrap();

        app.next_day();
        assert_eq!(app.selected_date, NaiveDate::from_ymd_opt(2026, 10, 11).unwrap());

        app.prev_day();
        assert_eq!(app.selected_date, NaiveDate::from_ymd_opt(2026, 10, 10).unwrap());

        app.prev_week();
        assert_eq!(app.selected_date, NaiveDate::from_ymd_opt(2026, 10, 3).unwrap());

        app.next_week();
        assert_eq!(app.selected_date, NaiveDate::from_ymd_opt(2026, 10, 10).unwrap());
    }
}
