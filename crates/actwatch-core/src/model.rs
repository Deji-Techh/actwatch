//! Shared event/sample structs used by every crate.
//!
//! `AppEvent` and `ResourceSample` are the daemon's write units and the
//! rollup-rebuild inputs. `title` is `Option<String>` so the daemon can
//! apply [`crate::TitlePolicy`] *before* writing: `None` is stored as `''`
//! (the `app_events.title` column stays `TEXT NOT NULL`, unchanged by the
//! v2 contract), `Some(t)` is stored verbatim.

/// One focused-window span (`ts` = unix seconds).
#[derive(Debug, Clone, PartialEq)]
pub struct AppEvent {
    pub ts: i64,
    pub app_id: String,
    /// Window title after [`crate::TitlePolicy`] redaction (`None` = redacted).
    pub title: Option<String>,
    pub dur_secs: i64,
}

impl AppEvent {
    pub fn new(
        ts: i64,
        app_id: impl Into<String>,
        title: Option<String>,
        dur_secs: i64,
    ) -> Self {
        Self {
            ts,
            app_id: app_id.into(),
            title,
            dur_secs,
        }
    }
}

/// One per-PID resource sample (`ts` = unix seconds).
#[derive(Debug, Clone, PartialEq)]
pub struct ResourceSample {
    pub ts: i64,
    pub app_id: String,
    pub pid: i64,
    pub cpu_pct: f64,
    pub rss_kb: i64,
    pub gpu_pct: f64,
}

impl ResourceSample {
    pub fn new(
        ts: i64,
        app_id: impl Into<String>,
        pid: i64,
        cpu_pct: f64,
        rss_kb: i64,
        gpu_pct: f64,
    ) -> Self {
        Self {
            ts,
            app_id: app_id.into(),
            pid,
            cpu_pct,
            rss_kb,
            gpu_pct,
        }
    }
}
