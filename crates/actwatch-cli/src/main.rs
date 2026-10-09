//! actwatch — read-only CLI over the local activity database.
//!
//! Module map:
//!   cli.rs      clap surface
//!   db.rs       read-only connection + path resolution
//!   sql.rs      every SQL statement, in one place
//!   model.rs    query results
//!   date.rs     day ranges and local-time boundaries
//!   format.rs   table / CSV / JSON rendering
//!   systemd.rs  systemctl --user helpers

mod cli;
mod date;
mod db;
mod format;
mod model;
mod sql;
mod systemd;

use std::path::Path;
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::Parser;

use cli::{Cli, Command, DaemonCommand, ExportArgs, QueryArgs, RangeArgs};
use date::DateRange;
use model::{RangeReport, Source};

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("actwatch: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: &Cli) -> Result<()> {
    match &cli.command {
        Command::Status => status(cli.db.as_deref()),
        Command::Query(args) => query(cli.db.as_deref(), args),
        Command::Export(args) => export(cli.db.as_deref(), args),
        Command::Daemon(cmd) => daemon(cmd),
    }
}

fn resolve_range(args: &RangeArgs) -> Result<DateRange> {
    date::resolve(
        args.day.as_deref(),
        args.week,
        args.since.as_deref(),
        args.until.as_deref(),
        args.app.clone(),
        args.source,
    )
}

fn load_report(db: Option<&Path>, range: &DateRange) -> Result<RangeReport> {
    let path = db::resolve_db_path(db);
    let conn = db::open_ready(&path)?;
    let today = date::format_day(&date::today());
    sql::build_report(
        &conn,
        &range.start_day(),
        &range.end_day(),
        &today,
        range.app_str(),
        range.source,
    )
}

fn query(db: Option<&Path>, args: &QueryArgs) -> Result<()> {
    let range = resolve_range(&args.range)?;
    let report = load_report(db, &range)?;
    println!("{}", format::render_table(&report, &range, args.limit));
    Ok(())
}

fn export(db: Option<&Path>, args: &ExportArgs) -> Result<()> {
    let range = resolve_range(&args.range)?;
    let report = load_report(db, &range)?;
    let report = if args.slim { report.slim() } else { report };

    if args.json {
        let json = format::render_json(&report, &range).context("failed to encode JSON")?;
        println!("{json}");
    } else {
        println!("{}", format::render_csv(&report));
    }
    Ok(())
}

/// Thin passthrough so `daemon start|stop` stays readable in scripts.
fn daemon(cmd: &DaemonCommand) -> Result<()> {
    let (action, unit, enable) = match cmd {
        DaemonCommand::Start(a) => ("start", a.unit.clone(), a.enable),
        DaemonCommand::Stop(a) => ("stop", a.unit.clone(), a.enable),
    };
    let unit = systemd::unit_name(&unit).to_string();
    println!("{}", systemd::run(&unit, action, enable)?);
    Ok(())
}

fn status(db: Option<&Path>) -> Result<()> {
    let path = db::resolve_db_path(db);
    let unit = std::env::var(db::ENV_UNIT).unwrap_or_else(|_| db::DEFAULT_UNIT.to_string());
    let mut rows: Vec<(&str, String)> = Vec::new();

    rows.push(("version", format!("v{}", format::VERSION)));
    rows.push(("database", path.display().to_string()));

    if !path.exists() {
        rows.push((
            "state",
            format::warning("missing — start actwatch-daemon to create it"),
        ));
        rows.push(("daemon", systemd::summary(&unit)));
        rows.push(("config", config_row()));
        println!("{}", format::heading("actwatch status"));
        println!("{}", format::render_pairs(&rows));
        return Ok(());
    }

    rows.push(("size", systemd::db_size_line(&path)));
    let conn = db::open(&path).with_context(|| format!("failed to read {}", path.display()))?;
    let present = sql::tables_present(&conn)?;

    if present.is_empty() {
        rows.push(("state", format::warning("no activity tables yet")));
    } else {
        rows.push(("tables", present.join(", ")));
    }

    if present.iter().any(|t| t == "meta") {
        let version = sql::meta_get(&conn, "schema_version")?;
        let cats = sql::meta_get(&conn, "category_map_version")?;
        match (version, cats) {
            (Some(v), Some(c)) => rows.push(("schema", format!("v{v}, categories v{c}"))),
            (Some(v), None) => rows.push(("schema", format!("v{v}"))),
            _ => {}
        }
    }

    if !present.is_empty() {
        let totals = sql::history_totals(&conn, &present)?;
        if totals.days > 0 {
            let span = match (&totals.first_day, &totals.last_day) {
                (Some(a), Some(b)) => format!("{a} .. {b}"),
                _ => "unknown".to_string(),
            };
            rows.push((
                "history",
                format!(
                    "{span} · {} day{} · {} tracked",
                    totals.days,
                    if totals.days == 1 { "" } else { "s" },
                    format::human_duration(totals.total_secs)
                ),
            ));
        }
    }

    let today_range = DateRange {
        from: date::today(),
        to: date::today(),
        app: None,
        source: Source::Auto,
    };
    match sql::build_report(
        &conn,
        &today_range.start_day(),
        &today_range.end_day(),
        &date::format_day(&date::today()),
        None,
        Source::Auto,
    ) {
        Ok(report) if !report.is_empty() => {
            let sources: Vec<&str> = report.sources().iter().map(|s| s.as_str()).collect();
            rows.push((
                "today",
                format!(
                    "{} tracked · {} app{} [{}]",
                    format::human_duration(report.total_secs()),
                    report.distinct_apps(),
                    if report.distinct_apps() == 1 { "" } else { "s" },
                    sources.join("+")
                ),
            ));
        }
        _ => rows.push(("today", "no activity recorded yet".into())),
    }

    rows.push(("daemon", systemd::summary(&unit)));
    rows.push(("config", config_row()));

    println!("{}", format::heading("actwatch status"));
    println!("{}", format::render_pairs(&rows));
    Ok(())
}

fn config_row() -> String {
    let path = db::config_path();
    if path.exists() {
        format!("{} (present)", path.display())
    } else {
        format!("{} (missing — using built-in defaults)", path.display())
    }
}
