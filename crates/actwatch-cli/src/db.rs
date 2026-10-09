//! Read-only connection handling.
//!
//! The CLI never writes to the database: it opens with `SQLITE_OPEN_READ_ONLY`
//! and pins `PRAGMA query_only` so that a bug cannot mutate or prune history.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use rusqlite::{Connection, OpenFlags};

pub const ENV_DB: &str = "ACTWATCH_DB";
pub const ENV_UNIT: &str = "ACTWATCH_UNIT";
pub const DEFAULT_UNIT: &str = "actwatch-daemon.service";

/// `$ACTWATCH_DB` → `--db` → `$XDG_DATA_HOME/actwatch/activity.db`.
pub fn resolve_db_path(explicit: Option<&Path>) -> PathBuf {
    if let Some(path) = explicit {
        return path.to_path_buf();
    }
    if let Ok(raw) = std::env::var(ENV_DB) {
        if !raw.trim().is_empty() {
            return PathBuf::from(raw);
        }
    }
    data_dir().join("actwatch").join("activity.db")
}

pub fn data_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_DATA_HOME") {
        if !dir.trim().is_empty() {
            return PathBuf::from(dir);
        }
    }
    home_dir().join(".local").join("share")
}

pub fn config_path() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_CONFIG_HOME") {
        if !dir.trim().is_empty() {
            return PathBuf::from(dir).join("actwatch").join("categories.toml");
        }
    }
    home_dir()
        .join(".config")
        .join("actwatch")
        .join("categories.toml")
}

pub fn home_dir() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/"))
}

fn not_initialised(path: &Path) -> anyhow::Error {
    anyhow!(
        "no actwatch database at {}\n\
         hint: start the collector once (`systemctl --user start {DEFAULT_UNIT}`) \
         or point --db at an existing activity.db",
        path.display()
    )
}

pub fn open(path: &Path) -> Result<Connection> {
    if !path.exists() {
        bail!(not_initialised(path));
    }
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("failed to open {} read-only", path.display()))?;
    let _ = conn.execute_batch("PRAGMA query_only = true;");
    Ok(conn)
}

/// Opens the database and verifies it is a usable actwatch store.
pub fn open_ready(path: &Path) -> Result<Connection> {
    let conn = open(path)?;
    let present = crate::sql::tables_present(&conn)?;
    if present.is_empty() {
        bail!(not_initialised(path));
    }
    if !present.iter().any(|t| t == "daily_rollup") && !present.iter().any(|t| t == "app_events") {
        bail!(
            "database {} has no activity tables yet (found: {})",
            path.display(),
            if present.is_empty() {
                "none".to_string()
            } else {
                present.join(", ")
            }
        );
    }
    Ok(conn)
}

pub fn file_size(path: &Path) -> Option<u64> {
    std::fs::metadata(path).ok().map(|m| m.len())
}
