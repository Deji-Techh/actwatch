use crate::away::is_lockscreen_class;
use crate::models::ActiveWindowInfo;
use std::path::PathBuf;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::process::Command;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

pub struct HyprlandManager {
    socket_path: Option<PathBuf>,
    socket2_path: Option<PathBuf>,
}

impl HyprlandManager {
    pub fn new() -> Self {
        let (socket_path, socket2_path) = Self::locate_sockets();
        Self {
            socket_path,
            socket2_path,
        }
    }

    pub fn locate_sockets() -> (Option<PathBuf>, Option<PathBuf>) {
        let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| {
            let uid = unsafe { libc::getuid() };
            format!("/run/user/{uid}")
        });

        let hypr_base = PathBuf::from(runtime_dir).join("hypr");

        // 1. Try env var HYPRLAND_INSTANCE_SIGNATURE
        if let Ok(sig) = std::env::var("HYPRLAND_INSTANCE_SIGNATURE") {
            if !sig.trim().is_empty() {
                let inst_dir = hypr_base.join(sig.trim());
                let s1 = inst_dir.join(".socket.sock");
                let s2 = inst_dir.join(".socket2.sock");
                if s1.exists() || s2.exists() {
                    return (Some(s1), Some(s2));
                }
            }
        }

        // 2. Discover newest instance directory in $XDG_RUNTIME_DIR/hypr/
        if let Ok(entries) = std::fs::read_dir(&hypr_base) {
            let mut candidates: Vec<PathBuf> = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect();

            // Sort by modification time descending
            candidates.sort_by_key(|p| {
                std::fs::metadata(p)
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
            });
            candidates.reverse();

            for dir in candidates {
                let s1 = dir.join(".socket.sock");
                let s2 = dir.join(".socket2.sock");
                if s1.exists() || s2.exists() {
                    return (Some(s1), Some(s2));
                }
            }
        }

        (None, None)
    }

    /// Query current active window info.
    /// Prefers direct unix socket to avoid spawning subprocesses;
    /// falls back to `hyprctl activewindow -j` or `hyprctl clients -j`.
    pub async fn query_active_window(&mut self) -> ActiveWindowInfo {
        if self.socket_path.is_none() || self.socket2_path.is_none() {
            let (s1, s2) = Self::locate_sockets();
            self.socket_path = s1;
            self.socket2_path = s2;
        }

        // 1. Try command socket .socket.sock (fastest, 0 subprocess forks)
        if let Some(ref s1_path) = self.socket_path {
            if let Ok(mut stream) = UnixStream::connect(s1_path).await {
                if stream.write_all(b"j/activewindow").await.is_ok() {
                    let mut buf = Vec::new();
                    if stream.read_to_end(&mut buf).await.is_ok() {
                        if let Ok(val) = serde_json::from_slice::<serde_json::Value>(&buf) {
                            if let Some(info) = Self::parse_window_json(&val) {
                                return info;
                            }
                        }
                    }
                }
            }
        }

        // 2. Fallback to hyprctl activewindow -j
        if let Ok(output) = Command::new("hyprctl")
            .args(["activewindow", "-j"])
            .output()
            .await
        {
            if output.status.success() {
                if let Ok(val) = serde_json::from_slice::<serde_json::Value>(&output.stdout) {
                    if let Some(info) = Self::parse_window_json(&val) {
                        return info;
                    }
                }
            }
        }

        // 3. Fallback to hyprctl clients -j (find focused client)
        if let Ok(output) = Command::new("hyprctl")
            .args(["clients", "-j"])
            .output()
            .await
        {
            if output.status.success() {
                if let Ok(val) = serde_json::from_slice::<serde_json::Value>(&output.stdout) {
                    if let Some(arr) = val.as_array() {
                        for item in arr {
                            if item.get("focusHistoryID").and_then(|v| v.as_i64()) == Some(0) {
                                if let Some(info) = Self::parse_window_json(item) {
                                    return info;
                                }
                            }
                        }
                    }
                }
            }
        }

        ActiveWindowInfo::away()
    }

    pub fn parse_window_json(val: &serde_json::Value) -> Option<ActiveWindowInfo> {
        let class = val
            .get("class")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();

        let title = val
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();

        let pid = val.get("pid").and_then(|v| v.as_u64()).map(|p| p as u32);

        // Check if idle or screen locked
        if class.is_empty() || pid.is_none() || pid == Some(0) || is_lockscreen_class(class) {
            return Some(ActiveWindowInfo::away());
        }

        Some(ActiveWindowInfo {
            app_id: class.to_string(),
            title: title.to_string(),
            pid,
        })
    }

    /// Run tracking loop: event-driven via socket2.sock with automatic 2s poll fallback.
    pub async fn run(mut self, tx: mpsc::Sender<ActiveWindowInfo>) {
        loop {
            if self.socket2_path.is_none() {
                let (s1, s2) = Self::locate_sockets();
                self.socket_path = s1;
                self.socket2_path = s2;
            }

            let s2_path = match &self.socket2_path {
                Some(p) if p.exists() => p.clone(),
                _ => {
                    self.run_poll_fallback(&tx).await;
                    continue;
                }
            };

            match UnixStream::connect(&s2_path).await {
                Ok(stream) => {
                    info!(
                        socket = %s2_path.display(),
                        "Connected to Hyprland event socket (socket2)"
                    );

                    let reader = BufReader::new(stream);
                    let mut lines = reader.lines();

                    // Send initial active window on connection
                    let initial = self.query_active_window().await;
                    let _ = tx.send(initial).await;

                    while let Ok(Some(line)) = lines.next_line().await {
                        if line.starts_with("activewindow>>") {
                            let data = &line["activewindow>>".len()..];
                            let win = if data.trim().is_empty() || data.trim() == "," {
                                ActiveWindowInfo::away()
                            } else {
                                self.query_active_window().await
                            };
                            if tx.send(win).await.is_err() {
                                return;
                            }
                        } else if line.starts_with("lock>>") {
                            debug!("Received lock event from Hyprland");
                            if tx.send(ActiveWindowInfo::away()).await.is_err() {
                                return;
                            }
                        } else if line.starts_with("unlock>>") {
                            debug!("Received unlock event from Hyprland");
                            let win = self.query_active_window().await;
                            if tx.send(win).await.is_err() {
                                return;
                            }
                        }
                    }

                    warn!("Hyprland socket2 connection closed; falling back to polling");
                }
                Err(e) => {
                    warn!(
                        error = %e,
                        path = %s2_path.display(),
                        "Cannot connect to Hyprland socket2; starting 2s poll fallback"
                    );
                    self.run_poll_fallback(&tx).await;
                }
            }

            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    /// Fallback polling loop: poll every 2s, periodically attempt to reconnect to socket2
    async fn run_poll_fallback(&mut self, tx: &mpsc::Sender<ActiveWindowInfo>) {
        info!("Running Hyprland active-window fallback poll (every 2s)");
        let mut last_window: Option<ActiveWindowInfo> = None;
        let mut poll_count: u32 = 0;

        loop {
            let win = self.query_active_window().await;
            if last_window.as_ref() != Some(&win) {
                last_window = Some(win.clone());
                if tx.send(win).await.is_err() {
                    return;
                }
            }

            tokio::time::sleep(Duration::from_secs(2)).await;
            poll_count += 1;

            // Every 10s (5 polls), check if socket2 has become available
            if poll_count % 5 == 0 {
                let (s1, s2) = Self::locate_sockets();
                if let Some(ref p) = s2 {
                    if p.exists() {
                        self.socket_path = s1;
                        self.socket2_path = s2;
                        info!("Hyprland socket2 discovered, switching back to event-driven mode");
                        return;
                    }
                }
            }
        }
    }
}

pub async fn query_active_window_oneshot() -> ActiveWindowInfo {
    let mut mgr = HyprlandManager::new();
    mgr.query_active_window().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_window_json_regular() {
        let val = serde_json::json!({
            "class": "firefox",
            "title": "Rust Programming",
            "pid": 4567
        });
        let info = HyprlandManager::parse_window_json(&val).unwrap();
        assert_eq!(info.app_id, "firefox");
        assert_eq!(info.title, "Rust Programming");
        assert_eq!(info.pid, Some(4567));
        assert!(!info.is_away());
    }

    #[test]
    fn test_parse_window_json_lockscreen() {
        let val = serde_json::json!({
            "class": "hyprlock",
            "title": "",
            "pid": 8888
        });
        let info = HyprlandManager::parse_window_json(&val).unwrap();
        assert_eq!(info.app_id, "away");
        assert!(info.is_away());
    }

    #[test]
    fn test_parse_window_json_empty() {
        let val = serde_json::json!({
            "class": "",
            "title": "",
            "pid": 0
        });
        let info = HyprlandManager::parse_window_json(&val).unwrap();
        assert_eq!(info.app_id, "away");
        assert!(info.is_away());
    }
}
