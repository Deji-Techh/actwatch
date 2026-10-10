use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Config {
    pub foreground: bool,
    pub db_path: PathBuf,
}

pub type Args = Config;

impl Config {
    pub fn parse() -> Result<Self, String> {
        let mut foreground = false;
        let mut db_path = None;

        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--foreground" | "-f" => {
                    foreground = true;
                }
                "--db" => {
                    if let Some(val) = args.next() {
                        db_path = Some(PathBuf::from(val));
                    } else {
                        return Err("--db requires a path argument".to_string());
                    }
                }
                arg if arg.starts_with("--db=") => {
                    let val = &arg["--db=".len()..];
                    db_path = Some(PathBuf::from(val));
                }
                "--help" | "-h" => {
                    println!("actwatch-daemon — 24/7 Hyprland activity collector\n");
                    println!("Usage: actwatch-daemon [OPTIONS]");
                    println!("\nOptions:");
                    println!("  -f, --foreground    Run in foreground (log to stderr)");
                    println!("      --db <PATH>     Custom SQLite database path (default: ~/.local/share/actwatch/activity.db)");
                    println!("  -h, --help          Show this help message");
                    std::process::exit(0);
                }
                unknown => {
                    return Err(format!("Unknown option: {unknown}"));
                }
            }
        }

        let db_path = db_path.unwrap_or_else(default_db_path);

        Ok(Config {
            foreground,
            db_path,
        })
    }
}

pub fn default_db_path() -> PathBuf {
    if let Ok(data_home) = std::env::var("XDG_DATA_HOME") {
        if !data_home.trim().is_empty() {
            return PathBuf::from(data_home).join("actwatch").join("activity.db");
        }
    }

    if let Ok(home) = std::env::var("HOME") {
        if !home.trim().is_empty() {
            return PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("actwatch")
                .join("activity.db");
        }
    }

    PathBuf::from("activity.db")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_db_path() {
        let path = default_db_path();
        assert!(path.ends_with("actwatch/activity.db") || path == PathBuf::from("activity.db"));
    }
}
