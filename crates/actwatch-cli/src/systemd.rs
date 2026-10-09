//! `systemctl --user` helpers.
//!
//! The CLI never becomes its own service manager: `daemon start|stop` simply
//! shells out to the user systemd manager, which is what actually keeps the
//! collector alive across suspend/resume.

use std::process::Command;

use crate::format::human_bytes;

#[derive(Debug, Clone)]
pub struct DaemonState {
    pub available: bool,
    pub detail: String,
    pub enabled: Option<String>,
}

/// Unit name used in messages.
pub fn unit_name(explicit: &str) -> &str {
    if explicit.trim().is_empty() {
        crate::db::DEFAULT_UNIT
    } else {
        explicit
    }
}

fn systemctl(args: &[&str]) -> Option<(bool, String)> {
    let output = Command::new("systemctl")
        .arg("--user")
        .args(args)
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Some((output.status.success(), text))
}

/// Reads `is-active` / `is-enabled` without ever failing the command.
pub fn state(unit: &str) -> DaemonState {
    if Command::new("systemctl")
        .arg("--version")
        .output()
        .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
    {
        return DaemonState {
            available: false,
            detail: "systemctl not found (not a systemd session)".into(),
            enabled: None,
        };
    }

    let (ok, active) = systemctl(&["is-active", unit]).unwrap_or((false, String::new()));
    let enabled = systemctl(&["is-enabled", unit]).map(|(_, text)| text);
    let detail = if active.is_empty() {
        if ok {
            "active".into()
        } else {
            "inactive".into()
        }
    } else {
        active
    };

    DaemonState {
        available: true,
        detail,
        enabled,
    }
}

/// Runs `systemctl --user start|stop <unit>`, enabling it when asked.
pub fn run(unit: &str, action: &str, enable: bool) -> anyhow::Result<String> {
    let mut args: Vec<&str> = Vec::with_capacity(3);
    match (action, enable) {
        ("start", true) => args.extend(["enable", "--now"]),
        ("start", false) => args.push("start"),
        _ => args.push("stop"),
    }
    args.push(unit);

    let output = Command::new("systemctl")
        .arg("--user")
        .args(&args)
        .output()
        .map_err(|e| anyhow::anyhow!("failed to run `systemctl --user {}`: {e}", args.join(" ")))?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if output.status.success() {
        let state = state(unit);
        let mut msg = format!(
            "systemctl --user {} → {} ({})",
            args.join(" "),
            state.detail,
            unit
        );
        if stdout.is_empty() {
            if !stderr.is_empty() {
                msg = format!("{msg}: {stderr}");
            }
        } else {
            msg = format!("{stdout}\n{msg}");
        }
        Ok(msg)
    } else {
        let detail = if stderr.is_empty() { stdout } else { stderr };
        Err(anyhow::anyhow!(
            "systemctl --user {} failed: {}",
            args.join(" "),
            if detail.is_empty() {
                "no output".to_string()
            } else {
                detail
            }
        ))
    }
}

/// Short one-line summary used by `status`.
pub fn summary(unit: &str) -> String {
    let state = state(unit);
    if !state.available {
        return state.detail;
    }
    match &state.enabled {
        Some(enabled) => format!("{} (enabled: {enabled})", state.detail),
        None => state.detail,
    }
}

/// `du`-style line for the database plus its WAL sidecar.
pub fn db_size_line(path: &std::path::Path) -> String {
    let main = crate::db::file_size(path).unwrap_or(0);
    let wal = sidecar_size(path, "-wal");
    if wal > 0 {
        format!(
            "{} on disk (+{} wal)",
            human_bytes(main + wal),
            human_bytes(wal)
        )
    } else {
        human_bytes(main)
    }
}

fn sidecar_size(path: &std::path::Path, suffix: &str) -> u64 {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    std::fs::metadata(std::path::PathBuf::from(name))
        .map(|m| m.len())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_name_falls_back_to_default() {
        assert_eq!(unit_name("  "), crate::db::DEFAULT_UNIT);
        assert_eq!(unit_name("custom.service"), "custom.service");
    }

    #[test]
    fn db_size_line_is_human_readable() {
        let line = db_size_line(std::path::Path::new("/nonexistent/activity.db"));
        assert!(line.contains("B") || line.contains("KiB"));
    }
}
