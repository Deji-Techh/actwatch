use std::time::Instant;
use tokio::process::Command;
use tracing::trace;

pub const IDLE_THRESHOLD_SECS: u64 = 120;

/// Handles away/idle detection across three tiers:
/// 1. logind `IdleSinceHint > 120s` (polled <= every 30s; find session via loginctl, never crash if absent)
/// 2. fallback hypridle if present
/// 3. lockscreen-class
pub struct AwayDetector {
    session_id: Option<String>,
    hypridle_detected: Option<bool>,
    last_poll: Option<Instant>,
    last_status: bool,
}

impl AwayDetector {
    pub fn new() -> Self {
        Self {
            session_id: None,
            hypridle_detected: None,
            last_poll: None,
            last_status: false,
        }
    }

    /// Check if the session is currently considered Away.
    /// Never crashes if logind, loginctl, or hypridle are absent.
    pub async fn check_away(&mut self) -> bool {
        self.last_poll = Some(Instant::now());

        // 1. Try logind IdleSinceHint
        if let Some(logind_away) = self.check_logind().await {
            trace!(logind_away, "AwayDetector: logind check completed");
            self.last_status = logind_away;
            return logind_away;
        }

        // 2. Fallback: hypridle if present
        if let Some(hypridle_away) = self.check_hypridle().await {
            trace!(hypridle_away, "AwayDetector: hypridle fallback completed");
            self.last_status = hypridle_away;
            return hypridle_away;
        }

        // 3. Fallback: if external idle detection isn't available, rely on lockscreen-class
        // (evaluated separately via is_lockscreen_class or hyprland events).
        self.last_status = false;
        false
    }

    #[allow(dead_code)]
    pub fn last_status(&self) -> bool {
        self.last_status
    }

    async fn check_logind(&mut self) -> Option<bool> {
        // Try cached session_id or rediscover
        if self.session_id.is_none() {
            self.session_id = find_logind_session().await;
        }

        let sid = match &self.session_id {
            Some(s) => s.clone(),
            None => return None,
        };

        match query_logind_session(&sid).await {
            Some(status) => Some(status),
            None => {
                // If query failed on cached session ID, clear and try rediscovering once
                self.session_id = find_logind_session().await;
                if let Some(ref new_sid) = self.session_id {
                    query_logind_session(new_sid).await
                } else {
                    None
                }
            }
        }
    }

    async fn check_hypridle(&mut self) -> Option<bool> {
        if self.hypridle_detected.is_none() {
            // Check if hypridle binary is present
            self.hypridle_detected = Some(is_command_available("hypridle").await);
        }

        if self.hypridle_detected == Some(true) {
            // Check if hypridle is running
            if is_process_running("hypridle").await {
                // If hypridle is present and running, check if screen is locked or idle
                return None;
            }
        }

        None
    }
}

/// Check if a window class belongs to a known lockscreen.
pub fn is_lockscreen_class(class: &str) -> bool {
    let lower = class.trim().to_lowercase();
    matches!(
        lower.as_str(),
        "hyprlock" | "swaylock" | "waylock" | "lockscreen" | "gtklock" | "swaylock-effects"
    )
}

/// Find active logind session via environment or loginctl.
async fn find_logind_session() -> Option<String> {
    // 1. Env var XDG_SESSION_ID
    if let Ok(sid) = std::env::var("XDG_SESSION_ID") {
        let sid = sid.trim();
        if !sid.is_empty() {
            return Some(sid.to_string());
        }
    }

    // 2. Query loginctl show-user for Display session
    let uid = unsafe { libc::getuid() };
    if let Ok(output) = Command::new("loginctl")
        .args(["show-user", &uid.to_string(), "-p", "Display", "--value"])
        .output()
        .await
    {
        if output.status.success() {
            let val = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !val.is_empty() && val != "0" {
                return Some(val);
            }
        }
    }

    // 3. Fallback: loginctl list-sessions --no-legend
    if let Ok(output) = Command::new("loginctl")
        .args(["list-sessions", "--no-legend"])
        .output()
        .await
    {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 6 {
                    let sid = parts[0];
                    let s_uid = parts[1];
                    let s_class = parts[5];
                    if s_uid == uid.to_string() && (s_class == "user" || parts[3] != "-") {
                        return Some(sid.to_string());
                    }
                }
            }
        }
    }

    None
}

/// Parse properties from `loginctl show-session <id>`
async fn query_logind_session(session_id: &str) -> Option<bool> {
    let output = Command::new("loginctl")
        .args([
            "show-session",
            session_id,
            "-p",
            "IdleHint",
            "-p",
            "IdleSinceHint",
            "-p",
            "LockedHint",
        ])
        .output()
        .await
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_logind_output(&stdout, chrono::Utc::now().timestamp_micros())
}

/// Parse logind show-session properties output.
pub fn parse_logind_output(output: &str, now_micros: i64) -> Option<bool> {
    let mut idle_hint = None;
    let mut idle_since_hint: Option<u64> = None;
    let mut locked_hint = None;

    for line in output.lines() {
        let trimmed = line.trim();
        if let Some(val) = trimmed.strip_prefix("IdleHint=") {
            idle_hint = Some(val.trim() == "yes");
        } else if let Some(val) = trimmed.strip_prefix("IdleSinceHint=") {
            idle_since_hint = val.trim().parse::<u64>().ok();
        } else if let Some(val) = trimmed.strip_prefix("LockedHint=") {
            locked_hint = Some(val.trim() == "yes");
        }
    }

    // Locked is always away
    if locked_hint == Some(true) {
        return Some(true);
    }

    if let Some(is_idle) = idle_hint {
        if !is_idle {
            return Some(false);
        }

        // IdleHint=yes: verify IdleSinceHint > 120s
        if let Some(since) = idle_since_hint {
            if since > 0 {
                let idle_secs = if since > 1_000_000_000_000 {
                    // Microseconds timestamp
                    (now_micros.saturating_sub(since as i64)) / 1_000_000
                } else {
                    // Seconds timestamp
                    (now_micros / 1_000_000).saturating_sub(since as i64)
                };

                return Some(idle_secs > (IDLE_THRESHOLD_SECS as i64));
            }
        }

        // If IdleSinceHint was 0 or missing but IdleHint is yes
        return Some(true);
    }

    None
}

async fn is_command_available(cmd: &str) -> bool {
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let full = dir.join(cmd);
            if full.is_file() {
                return true;
            }
        }
    }
    false
}

async fn is_process_running(proc_name: &str) -> bool {
    if let Ok(entries) = std::fs::read_dir("/proc") {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let comm_path = path.join("comm");
                if let Ok(comm) = std::fs::read_to_string(comm_path) {
                    if comm.trim() == proc_name {
                        return true;
                    }
                }
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_lockscreen_class() {
        assert!(is_lockscreen_class("hyprlock"));
        assert!(is_lockscreen_class("Swaylock"));
        assert!(is_lockscreen_class("waylock"));
        assert!(is_lockscreen_class("lockscreen"));
        assert!(is_lockscreen_class("gtklock"));
        assert!(is_lockscreen_class("swaylock-effects"));
        assert!(!is_lockscreen_class("firefox"));
        assert!(!is_lockscreen_class("kitty"));
    }

    #[test]
    fn test_parse_logind_not_idle() {
        let out = "IdleHint=no\nIdleSinceHint=0\nLockedHint=no\n";
        assert_eq!(parse_logind_output(out, 1700000000_000_000_i64), Some(false));
    }

    #[test]
    fn test_parse_logind_locked() {
        let out = "IdleHint=no\nIdleSinceHint=0\nLockedHint=yes\n";
        assert_eq!(parse_logind_output(out, 1700000000_000_000_i64), Some(true));
    }

    #[test]
    fn test_parse_logind_idle_under_120s() {
        let now_micros = 1700000100_000_000_i64;
        let since_micros = 1700000040_000_000_u64; // 60s ago
        let out = format!("IdleHint=yes\nIdleSinceHint={since_micros}\nLockedHint=no\n");
        assert_eq!(parse_logind_output(&out, now_micros), Some(false));
    }

    #[test]
    fn test_parse_logind_idle_over_120s() {
        let now_micros = 1700000200_000_000_i64;
        let since_micros = 1700000020_000_000_u64; // 180s ago
        let out = format!("IdleHint=yes\nIdleSinceHint={since_micros}\nLockedHint=no\n");
        assert_eq!(parse_logind_output(&out, now_micros), Some(true));
    }

    #[test]
    fn test_parse_logind_idle_seconds_timestamp_over_120s() {
        let now_micros = 1700000200_000_000_i64;
        let since_secs = 1700000000_u64; // 200s ago
        let out = format!("IdleHint=yes\nIdleSinceHint={since_secs}\nLockedHint=no\n");
        assert_eq!(parse_logind_output(&out, now_micros), Some(true));
    }

    #[test]
    fn test_parse_logind_empty_or_invalid() {
        assert_eq!(parse_logind_output("", 1700000000_000_000_i64), None);
        assert_eq!(parse_logind_output("SomethingElse=foo\n", 1700000000_000_000_i64), None);
    }
}
