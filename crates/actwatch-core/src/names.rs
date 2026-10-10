//! Friendly display names: `core::names::DisplayNames`.
//!
//! `DisplayNames::load()` scans `/usr/share/applications` and
//! `~/.local/share/applications` for `*.desktop` files, indexing `Name=`
//! under both the `StartupWMClass` value (case-insensitive) and the bare
//! filename stem, matched against the Hyprland app-id.
//!
//! `resolve(app_id)` order: (1) `[aliases]` override from
//! `~/.config/actwatch/categories.toml`, (2) desktop-file match,
//! (3) prettify: strip a reverse-DNS prefix (`com.t3tools.T3Code` →
//! `T3Code`), replace `-_.` with spaces, uppercase the first letter of each
//! word (remainder kept verbatim so `T3Code` keeps its case).
//! The reserved id `"away"` (any case) always resolves to `"Away"`.

use anyhow::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Default system + user `.desktop` scan dirs.
fn default_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![PathBuf::from("/usr/share/applications")];
    if let Ok(home) = std::env::var("HOME") {
        if !home.is_empty() {
            dirs.push(PathBuf::from(home).join(".local/share/applications"));
        }
    }
    dirs
}

/// Default config path carrying the `[aliases]` table.
fn default_config_path() -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .filter(|h| !h.is_empty())
        .map(|h| PathBuf::from(h).join(".config/actwatch/categories.toml"))
}

/// Maps Hyprland app-ids to friendly display names.
#[derive(Debug, Clone, Default)]
pub struct DisplayNames {
    /// Lowercased app-id → alias (from `[aliases]`, highest priority).
    aliases: HashMap<String, String>,
    /// Lowercased app-id (`StartupWMClass` or filename stem) → `Name=`.
    entries: HashMap<String, String>,
}

impl DisplayNames {
    /// Scan the default app dirs and read `[aliases]` from the default
    /// config path. Never fails hard: missing dirs/files yield an empty map.
    pub fn load() -> Self {
        let dirs = default_dirs();
        let refs: Vec<&Path> = dirs.iter().map(|p| p.as_path()).collect();
        let aliases = default_config_path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| Self::parse_aliases(&s).ok())
            .unwrap_or_default();
        Self::load_from_dirs(&refs, aliases)
    }

    /// Build from an `[aliases]` TOML string plus the default app dirs.
    pub fn from_toml_str(s: &str) -> Result<Self> {
        let aliases = Self::parse_aliases(s)?;
        let dirs = default_dirs();
        let refs: Vec<&Path> = dirs.iter().map(|p| p.as_path()).collect();
        Ok(Self::load_from_dirs(&refs, aliases))
    }

    /// Build from explicit scan dirs + a pre-parsed alias map (tests).
    pub fn load_from_dirs(dirs: &[&Path], aliases: HashMap<String, String>) -> Self {
        let mut entries = HashMap::new();
        for dir in dirs {
            Self::scan_dir(dir, &mut entries);
        }
        Self { aliases, entries }
    }

    /// Merge `[aliases]` overrides from a TOML string into `self`.
    pub fn apply_toml_str(&mut self, s: &str) -> Result<()> {
        for (k, v) in Self::parse_aliases(s)? {
            self.aliases.insert(k, v);
        }
        Ok(())
    }

    fn parse_aliases(s: &str) -> Result<HashMap<String, String>> {
        #[derive(serde::Deserialize, Default)]
        struct AliasToml {
            #[serde(default)]
            aliases: HashMap<String, String>,
        }
        let parsed: AliasToml =
            toml::from_str(s).map_err(|e| anyhow::anyhow!("parse aliases TOML: {e}"))?;
        Ok(parsed
            .aliases
            .into_iter()
            .map(|(k, v)| (k.to_lowercase(), v))
            .collect())
    }

    fn scan_dir(dir: &Path, entries: &mut HashMap<String, String>) {
        let read_dir = match std::fs::read_dir(dir) {
            Ok(rd) => rd,
            Err(_) => return,
        };
        let mut files: Vec<PathBuf> = read_dir
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "desktop"))
            .collect();
        files.sort();
        for path in files {
            if let Some((stem, name, wm_class)) = parse_desktop_file(&path) {
                // Filename stem (bare, without `.desktop`), lowercased.
                entries.entry(stem.to_lowercase()).or_insert_with(|| name.clone());
                // StartupWMClass match is case-insensitive.
                if let Some(wm) = wm_class {
                    entries.entry(wm.to_lowercase()).or_insert(name.clone());
                }
            }
        }
    }

    /// Resolve an app-id to its display name.
    ///
    /// (1) `[aliases]` override, (2) reserved `"away"` → `"Away"`,
    /// (3) desktop-file `Name=`, (4) [`DisplayNames::prettify`].
    pub fn resolve(&self, app_id: &str) -> String {
        let key = app_id.to_lowercase();
        if let Some(alias) = self.aliases.get(&key) {
            return alias.clone();
        }
        if key == "away" {
            return "Away".to_string();
        }
        if let Some(name) = self.entries.get(&key) {
            return name.clone();
        }
        Self::prettify(app_id)
    }

    /// Fallback prettifier: strip a reverse-DNS prefix (take after the
    /// last `.`), split on `-_.`/whitespace, uppercase each word's first
    /// letter (rest of the word kept verbatim).
    pub fn prettify(app_id: &str) -> String {
        let base = app_id.rsplit('.').next().unwrap_or(app_id);
        let spaced = base.replace(['-', '_', '.'], " ");
        let words: Vec<String> = spaced
            .split_whitespace()
            .map(|w| {
                let mut chars = w.chars();
                match chars.next() {
                    None => String::new(),
                    Some(first) => {
                        let upper: String = first.to_uppercase().collect();
                        upper + chars.as_str()
                    }
                }
            })
            .collect();
        let joined = words.join(" ");
        if joined.is_empty() {
            app_id.to_string()
        } else {
            joined
        }
    }
}

/// Parse one `.desktop` file → (filename stem, `Name=`, `StartupWMClass=`).
fn parse_desktop_file(path: &Path) -> Option<(String, String, Option<String>)> {
    let stem = path.file_stem()?.to_string_lossy().into_owned();
    let content = std::fs::read_to_string(path).ok()?;
    let mut name: Option<String> = None;
    let mut wm_class: Option<String> = None;
    for line in content.lines() {
        let line = line.trim();
        if name.is_none()
            && (line.starts_with("Name=") || line.starts_with("Name["))
        {
            if let Some(v) = line.split_once('=').map(|(_, v)| v.trim()) {
                if !v.is_empty() {
                    name = Some(v.to_string());
                }
            }
        } else if wm_class.is_none() && line.starts_with("StartupWMClass=") {
            if let Some(v) = line.split_once('=').map(|(_, v)| v.trim()) {
                if !v.is_empty() {
                    wm_class = Some(v.to_string());
                }
            }
        }
        if name.is_some() && wm_class.is_some() {
            break;
        }
    }
    name.map(|n| (stem, n, wm_class))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_desktop(dir: &Path, file: &str, body: &str) {
        let mut f = std::fs::File::create(dir.join(file)).unwrap();
        f.write_all(body.as_bytes()).unwrap();
    }

    fn fixture_dir(tag: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!(
            "actwatch-names-test-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        base
    }

    fn load_with(files: &[(&str, &str)], aliases_toml: &str) -> (PathBuf, DisplayNames) {
        let dir = fixture_dir("alias");
        for (file, body) in files {
            write_desktop(&dir, file, body);
        }
        let refs = [dir.as_path()];
        let aliases = DisplayNames::parse_aliases(aliases_toml).unwrap();
        let names = DisplayNames::load_from_dirs(&refs, aliases);
        (dir, names)
    }

    #[test]
    fn stem_and_wmclass_matching_is_case_insensitive() {
        let dir = fixture_dir("wm");
        write_desktop(
            &dir,
            "firefox.desktop",
            "[Desktop Entry]\nName=Firefox Web Browser\nStartupWMClass=firefox\n",
        );
        write_desktop(
            &dir,
            "code.desktop",
            "[Desktop Entry]\nName=Visual Studio Code\nStartupWMClass=Code\n",
        );
        let refs = [dir.as_path()];
        let names = DisplayNames::load_from_dirs(&refs, HashMap::new());
        // Filename stem match.
        assert_eq!(names.resolve("firefox"), "Firefox Web Browser");
        // StartupWMClass match, case-insensitive.
        assert_eq!(names.resolve("code"), "Visual Studio Code");
        assert_eq!(names.resolve("CODE"), "Visual Studio Code");
        assert_eq!(names.resolve("Firefox"), "Firefox Web Browser");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn aliases_override_desktop_and_prettify() {
        let (dir, names) = load_with(
            &[(
                "spotify.desktop",
                "[Desktop Entry]\nName=Spotify\nStartupWMClass=spotify\n",
            )],
            "[aliases]\nspotify = \"Music\"\nmyapp = \"Custom\"\n",
        );
        assert_eq!(names.resolve("spotify"), "Music");
        assert_eq!(names.resolve("SPOTIFY"), "Music");
        assert_eq!(names.resolve("myapp"), "Custom");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn away_resolves_to_away() {
        let names = DisplayNames::default();
        assert_eq!(names.resolve("away"), "Away");
        assert_eq!(names.resolve("AWAY"), "Away");
    }

    #[test]
    fn prettify_strips_reverse_dns_and_title_cases() {
        assert_eq!(DisplayNames::prettify("com.t3tools.T3Code"), "T3Code");
        assert_eq!(DisplayNames::prettify("google-chrome"), "Google Chrome");
        assert_eq!(DisplayNames::prettify("my_cool.app"), "App");
        assert_eq!(DisplayNames::prettify("firefox"), "Firefox");
        // Unknown ids fall back to prettify via resolve.
        let names = DisplayNames::default();
        assert_eq!(names.resolve("com.t3tools.T3Code"), "T3Code");
    }
}
