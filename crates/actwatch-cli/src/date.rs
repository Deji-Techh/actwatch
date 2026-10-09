//! Date parsing and day-boundary helpers shared by the CLI commands.

use anyhow::{anyhow, Context, Result};
use chrono::{Local, NaiveDate, TimeZone};

pub const DAY_FMT: &str = "%Y-%m-%d";

/// Guards against accidental multi-decade scans from a mistyped `--since`.
pub const MAX_RANGE_DAYS: i64 = 3660;

pub fn today() -> NaiveDate {
    Local::now().date_naive()
}

pub fn parse_day(value: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(value, DAY_FMT)
        .with_context(|| format!("invalid date `{value}`, expected YYYY-MM-DD"))
}

pub fn format_day(day: &NaiveDate) -> String {
    day.format(DAY_FMT).to_string()
}

fn local_epoch(day: &NaiveDate, hour: u32) -> Result<i64> {
    let naive = day
        .and_hms_opt(hour, 0, 0)
        .ok_or_else(|| anyhow!("invalid time for {day}"))?;
    Local
        .from_local_datetime(&naive)
        .earliest()
        .ok_or_else(|| anyhow!("{naive} does not exist in the local timezone"))
        .map(|dt| dt.timestamp())
}

pub fn day_start_epoch(day: &str) -> Result<i64> {
    local_epoch(&parse_day(day)?, 0)
}

pub fn day_start_epoch_after(day: &str) -> Result<i64> {
    let next = parse_day(day)?
        .succ_opt()
        .with_context(|| format!("date overflows after {day}"))?;
    local_epoch(&next, 0)
}

/// An inclusive `YYYY-MM-DD` range resolved from the CLI flags.
#[derive(Debug, Clone)]
pub struct DateRange {
    pub from: NaiveDate,
    pub to: NaiveDate,
    pub app: Option<String>,
    pub source: crate::model::Source,
}

impl DateRange {
    pub fn start_day(&self) -> String {
        format_day(&self.from)
    }

    pub fn end_day(&self) -> String {
        format_day(&self.to)
    }

    pub fn app_str(&self) -> Option<&str> {
        self.app.as_deref()
    }

    pub fn span_days(&self) -> i64 {
        self.to.signed_duration_since(self.from).num_days() + 1
    }

    /// Relative description used as the report title.
    pub fn label(&self) -> String {
        if self.from == self.to {
            format_day(&self.from)
        } else {
            format!("{} .. {}", self.start_day(), self.end_day())
        }
    }

    /// Fails when the caller inverted the range or asked for an absurd scan.
    pub fn validate(&self) -> Result<()> {
        if self.from > self.to {
            return Err(anyhow!(
                "range is inverted: {} is after {}",
                self.start_day(),
                self.end_day()
            ));
        }
        let span = self.span_days();
        if span > MAX_RANGE_DAYS {
            return Err(anyhow!(
                "range spans {span} days, which is over the {MAX_RANGE_DAYS}-day limit"
            ));
        }
        Ok(())
    }
}

/// Resolves `--day` / `--week` / `--since` / `--until` into one range.
///
/// With no range flag the range is today, which keeps
/// `actwatch export --json` a cheap machine-readable "today so far" feed.
pub fn resolve(
    day: Option<&str>,
    week: bool,
    since: Option<&str>,
    until: Option<&str>,
    app: Option<String>,
    source: crate::model::Source,
) -> Result<DateRange> {
    let today = today();

    let (from, to) = if let Some(day) = day {
        let d = parse_day(day)?;
        (d, d)
    } else if week {
        let from = today
            .checked_sub_days(chrono::Days::new(6))
            .ok_or_else(|| anyhow!("date underflows for the current month"))?;
        (from, today)
    } else if since.is_some() || until.is_some() {
        (
            match since {
                Some(s) => parse_day(s)?,
                None => today,
            },
            match until {
                Some(u) => parse_day(u)?,
                None => today,
            },
        )
    } else {
        (today, today)
    };

    let range = DateRange {
        from,
        to,
        app,
        source,
    };
    range.validate()?;
    Ok(range)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn like_safe_dates_roundtrip() {
        let d = NaiveDate::from_ymd_opt(2026, 2, 28).unwrap();
        assert_eq!(format_day(&d), "2026-02-28");
        assert_eq!(parse_day("2026-02-28").unwrap(), d);
    }

    #[test]
    fn rejects_bad_date() {
        assert!(parse_day("2026-13-01").is_err());
        assert!(parse_day("not-a-date").is_err());
    }

    #[test]
    fn default_range_is_today() {
        let r = resolve(None, false, None, None, None, crate::model::Source::Auto).unwrap();
        assert_eq!(r.from, today());
        assert_eq!(r.to, today());
        assert_eq!(r.span_days(), 1);
    }

    #[test]
    fn week_is_seven_days() {
        let r = resolve(None, true, None, None, None, crate::model::Source::Auto).unwrap();
        assert_eq!(r.span_days(), 7);
        assert_eq!(r.to, today());
    }

    #[test]
    fn rejects_inverted_and_huge_ranges() {
        let inverted = DateRange {
            from: NaiveDate::from_ymd_opt(2026, 5, 2).unwrap(),
            to: NaiveDate::from_ymd_opt(2026, 5, 1).unwrap(),
            app: None,
            source: crate::model::Source::Auto,
        };
        assert!(inverted.validate().is_err());

        let huge = DateRange {
            from: NaiveDate::from_ymd_opt(2000, 1, 1).unwrap(),
            to: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            app: None,
            source: crate::model::Source::Auto,
        };
        assert!(huge.validate().is_err());
    }

    #[test]
    fn day_boundaries_are_contiguous() {
        let start = day_start_epoch("2026-03-08").unwrap();
        let end = day_start_epoch_after("2026-03-08").unwrap();
        assert!(end > start);
        let naive = Local
            .timestamp_opt(end, 0)
            .single()
            .unwrap()
            .with_timezone(&Local);
        assert_eq!(naive.date_naive().format(DAY_FMT).to_string(), "2026-03-09");
    }
}
