#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppEvent {
    pub ts: i64,
    pub app_id: String,
    pub title: String,
    pub dur_secs: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResourceSample {
    pub ts: i64,
    pub app_id: String,
    pub pid: u32,
    pub cpu_pct: f64,
    pub rss_kb: u64,
    pub gpu_pct: f64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveWindowInfo {
    pub app_id: String,
    pub title: String,
    pub pid: Option<u32>,
}

impl ActiveWindowInfo {
    pub fn idle() -> Self {
        Self {
            app_id: "idle".to_string(),
            title: "idle".to_string(),
            pid: None,
        }
    }

    #[allow(dead_code)]
    pub fn is_idle(&self) -> bool {
        self.app_id == "idle" || self.app_id.is_empty()
    }
}
