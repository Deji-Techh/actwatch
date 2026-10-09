use rusqlite::{Connection, Result, OpenFlags};
use chrono::{NaiveDate, Datelike, Local, Duration};
use std::path::PathBuf;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct AppUsage {
    pub app_id: String,
    pub time_secs: u64,
    pub avg_cpu: f64,
    pub peak_rss_kb: u64,
    pub avg_gpu: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Category {
    Social,
    Productivity,
    Utilities,
    Uncategorized,
}

impl Category {
    pub fn from_app_id(id: &str) -> Self {
        let id_lower = id.to_lowercase();
        if id_lower.contains("discord") || id_lower.contains("telegram") || id_lower.contains("slack") {
            Category::Social
        } else if id_lower.contains("code") || id_lower.contains("nvim") || id_lower.contains("term") || id_lower.contains("browser") || id_lower.contains("firefox") || id_lower.contains("chrome") {
            Category::Productivity
        } else if id_lower.contains("top") || id_lower.contains("sys") || id_lower.contains("monitor") {
            Category::Utilities
        } else {
            Category::Uncategorized
        }
    }
}

pub struct Db {
    conn: Connection,
}

impl Db {
    pub fn open() -> Result<Self> {
        let mut path = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("~/.local/share"));
        path.push("actwatch");
        path.push("activity.db");
        
        let conn = match Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY) {
            Ok(c) => c,
            Err(_) => {
                let conn = Connection::open_in_memory()?;
                conn.execute_batch(
                    "CREATE TABLE IF NOT EXISTS daily_rollup(day TEXT, app_id TEXT, secs INTEGER, avg_cpu REAL, peak_rss_kb INTEGER, avg_gpu REAL);"
                )?;
                conn
            }
        };
        Ok(Self { conn })
    }

    pub fn get_day_usage(&self, date: NaiveDate) -> Result<Vec<AppUsage>> {
        let day_str = date.format("%Y-%m-%d").to_string();
        let mut stmt = match self.conn.prepare(
            "SELECT app_id, SUM(secs), AVG(avg_cpu), MAX(peak_rss_kb), AVG(avg_gpu) 
             FROM daily_rollup 
             WHERE day = ? 
             GROUP BY app_id"
        ) {
            Ok(s) => s,
            Err(_) => return Ok(Vec::new()),
        };
        
        let rows = stmt.query_map([day_str], |row| {
            Ok(AppUsage {
                app_id: row.get(0)?,
                time_secs: row.get::<_, i64>(1).unwrap_or(0) as u64,
                avg_cpu: row.get::<_, f64>(2).unwrap_or(0.0),
                peak_rss_kb: row.get::<_, i64>(3).unwrap_or(0) as u64,
                avg_gpu: row.get::<_, f64>(4).unwrap_or(0.0),
            })
        })?;

        let mut usage = Vec::new();
        for r in rows {
            if let Ok(u) = r {
                usage.push(u);
            }
        }
        
        // Sort by time descending
        usage.sort_by(|a, b| b.time_secs.cmp(&a.time_secs));
        Ok(usage)
    }

    pub fn get_weekly_summary(&self, end_date: NaiveDate) -> Result<Vec<(NaiveDate, HashMap<Category, u64>)>> {
        let mut week = Vec::new();
        for i in (0..7).rev() {
            let d = end_date - Duration::days(i);
            let day_str = d.format("%Y-%m-%d").to_string();
            let mut summary = HashMap::new();
            summary.insert(Category::Social, 0);
            summary.insert(Category::Productivity, 0);
            summary.insert(Category::Utilities, 0);
            summary.insert(Category::Uncategorized, 0);

            if let Ok(mut stmt) = self.conn.prepare("SELECT app_id, SUM(secs) FROM daily_rollup WHERE day = ? GROUP BY app_id") {
                if let Ok(rows) = stmt.query_map([day_str], |row| {
                    let app_id: String = row.get(0)?;
                    let secs: i64 = row.get(1)?;
                    Ok((app_id, secs as u64))
                }) {
                    for r in rows {
                        if let Ok((app_id, secs)) = r {
                            let cat = Category::from_app_id(&app_id);
                            *summary.entry(cat).or_insert(0) += secs;
                        }
                    }
                }
            }
            week.push((d, summary));
        }
        Ok(week)
    }
}
