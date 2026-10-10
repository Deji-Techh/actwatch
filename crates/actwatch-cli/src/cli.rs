//! Command-line surface for `actwatch`.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

use crate::model::Source;

const LONG_ABOUT: &str = "\
actwatch — read-only view over your local activity history.

Every subcommand opens ~/.local/share/actwatch/activity.db read-only.
Nothing is ever written, migrated or pruned by this tool: history is kept
forever, and only actwatch-daemon writes to the database.

Output is plain monochrome and app-level: apps are listed by their friendly
name, and window titles are never shown. Idle time is reported as its own
`Away` row and is left out of the app totals.

Typical use:
  actwatch status                  health of the collector + database
  actwatch query --day            today so far, per app
  actwatch query --week           last 7 days, per day
  actwatch query --app firefox    one app across today
  actwatch export --csv           spreadsheet-friendly rows
  actwatch export --json          machine-readable rows (today by default)
  actwatch daemon start           systemctl --user start actwatch-daemon";

#[derive(Debug, Parser)]
#[command(
    name = "actwatch",
    version,
    about = "read-only CLI for the actwatch activity database",
    long_about = LONG_ABOUT,
    propagate_version = true
)]
pub struct Cli {
    /// Path to activity.db [$ACTWATCH_DB]
    #[arg(long, short = 'd', global = true, value_name = "PATH")]
    pub db: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Show database, schema and daemon health
    Status,

    /// Show tracked time per day and per app
    Query(QueryArgs),

    /// Write rows as CSV or JSON
    Export(ExportArgs),

    /// Thin wrapper around `systemctl --user`
    #[command(subcommand)]
    Daemon(DaemonCommand),
}

#[derive(Debug, Args)]
pub struct QueryArgs {
    #[command(flatten)]
    pub range: RangeArgs,

    /// Cap apps listed per day (0 = no cap)
    #[arg(long, short = 'n', default_value_t = 20, value_name = "N")]
    pub limit: usize,
}

#[derive(Debug, Args)]
#[command(group(
    clap::ArgGroup::new("format").required(true).args(["csv", "json"]),
))]
pub struct ExportArgs {
    /// Emit CSV
    #[arg(long)]
    pub csv: bool,

    /// Emit JSON
    #[arg(long)]
    pub json: bool,

    /// Trim the CPU/RAM/GPU columns (CSV and JSON)
    #[arg(long)]
    pub slim: bool,

    #[command(flatten)]
    pub range: RangeArgs,
}

/// Shared day-range selection. With no flag at all the range is today.
#[derive(Debug, Args, Clone)]
pub struct RangeArgs {
    /// Single day, YYYY-MM-DD (bare `--day` means today)
    #[arg(long, value_name = "YYYY-MM-DD", num_args(0..=1), default_missing_value = "today", conflicts_with_all = ["week", "since", "until"])]
    pub day: Option<String>,

    /// Rolling 7 days ending today
    #[arg(long, conflicts_with_all = ["since", "until"])]
    pub week: bool,

    /// First day of the range, YYYY-MM-DD
    #[arg(long, value_name = "YYYY-MM-DD")]
    pub since: Option<String>,

    /// Last day of the range, YYYY-MM-DD (inclusive)
    #[arg(long, value_name = "YYYY-MM-DD")]
    pub until: Option<String>,

    /// Only this app (case-insensitive substring on the app id, then on the
    /// friendly name)
    #[arg(long, value_name = "APP")]
    pub app: Option<String>,

    /// Which table to read: auto mixes rollup (past) + live events (today)
    #[arg(long, default_value = "auto", value_name = "SOURCE")]
    pub source: Source,
}

#[derive(Debug, Subcommand)]
pub enum DaemonCommand {
    /// Start the user service via systemctl
    Start(DaemonAction),

    /// Stop the user service via systemctl
    Stop(DaemonAction),
}

#[derive(Debug, Args)]
pub struct DaemonAction {
    /// systemd unit to act on [$ACTWATCH_UNIT]
    #[arg(long, value_name = "UNIT", default_value = crate::db::DEFAULT_UNIT)]
    pub unit: String,

    /// Also enable the unit for future logins
    #[arg(long)]
    pub enable: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn export_requires_a_format() {
        assert!(Cli::try_parse_from(["actwatch", "export"]).is_err());
        assert!(Cli::try_parse_from(["actwatch", "export", "--csv"]).is_ok());
        assert!(Cli::try_parse_from(["actwatch", "export", "--json"]).is_ok());
        assert!(Cli::try_parse_from(["actwatch", "export", "--csv", "--json"]).is_err());
    }

    #[test]
    fn day_and_week_are_mutually_exclusive() {
        assert!(
            Cli::try_parse_from(["actwatch", "query", "--day", "2026-10-10", "--week"]).is_err()
        );
        assert!(Cli::try_parse_from(["actwatch", "query", "--day", "2026-10-10"]).is_ok());
        assert!(Cli::try_parse_from(["actwatch", "query", "--week"]).is_ok());
    }

    #[test]
    fn unknown_source_is_rejected() {
        assert!(Cli::try_parse_from(["actwatch", "query", "--source", "nope"]).is_err());
    }

    #[test]
    fn db_flag_is_global() {
        let cli = Cli::try_parse_from(["actwatch", "--db", "/tmp/x.db", "status"]).unwrap();
        assert_eq!(cli.db, Some(PathBuf::from("/tmp/x.db")));
        let cli = Cli::try_parse_from(["actwatch", "status", "--db", "/tmp/y.db"]).unwrap();
        assert_eq!(cli.db, Some(PathBuf::from("/tmp/y.db")));
    }

    #[test]
    fn default_unit_is_the_daemon_service() {
        let cli = Cli::try_parse_from(["actwatch", "daemon", "start"]).unwrap();
        match cli.command {
            Command::Daemon(DaemonCommand::Start(action)) => {
                assert_eq!(action.unit, "actwatch-daemon.service");
                assert!(!action.enable);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }
}
