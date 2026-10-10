//! App categories: TOML-driven mapping with builtin defaults.
//!
//! Canonical categories (match the dashboard legend):
//! `Social`, `Productivity & Finance`, `Utilities`, `Gaming`, `Media`,
//! `Other` (fallback).
//!
//! TOML format (user config overrides/merges over the builtins). The
//! canonical table is `[categories]`; `[apps]` is accepted as a legacy
//! alias for the same table:
//!
//! ```toml
//! [categories]
//! firefox = "Social"
//! code = "Productivity & Finance"
//! steam = "Gaming"
//! ```

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::collections::HashMap;

/// Canonical dashboard category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Category {
    Social,
    ProductivityFinance,
    Utilities,
    Gaming,
    Media,
    Other,
}

impl Category {
    /// All categories in legend order.
    pub fn all() -> [Category; 6] {
        [
            Category::Social,
            Category::ProductivityFinance,
            Category::Utilities,
            Category::Gaming,
            Category::Media,
            Category::Other,
        ]
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Category::Social => "Social",
            Category::ProductivityFinance => "Productivity & Finance",
            Category::Utilities => "Utilities",
            Category::Gaming => "Gaming",
            Category::Media => "Media",
            Category::Other => "Other",
        }
    }

    /// Parse a category name; accepts common aliases, case-insensitive.
    pub fn parse(s: &str) -> Result<Self> {
        match s.trim().to_lowercase().as_str() {
            "social" => Ok(Category::Social),
            "productivity & finance" | "productivity and finance" | "productivity"
            | "productivity_finance" | "productivity-finance" | "finance" => {
                Ok(Category::ProductivityFinance)
            }
            "utilities" | "utility" => Ok(Category::Utilities),
            "gaming" | "games" | "game" => Ok(Category::Gaming),
            "media" | "multimedia" | "video" | "music" => Ok(Category::Media),
            "other" | "misc" | "miscellaneous" | "uncategorized" => Ok(Category::Other),
            _ => bail!("unknown category: {s:?}"),
        }
    }
}

impl std::fmt::Display for Category {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Deserialize, Default)]
struct CategoryToml {
    /// Override table, two accepted shapes (both may appear):
    ///
    /// ```toml
    /// [categories]
    /// firefox = "Social"              # map style: app id -> category
    /// media = ["mpv", "vlc"]          # inline list: category -> apps
    /// [categories.gaming]
    /// apps = ["steam", "lutris"]      # section style: category -> apps
    /// ```
    #[serde(default)]
    categories: HashMap<String, toml::Value>,
    /// Legacy alias for `[categories]` (pre-v2 config files, map style only).
    #[serde(default)]
    apps: HashMap<String, String>,
}

/// Resolves an `app_id` to a [`Category`].
///
/// Starts from [`Categories::builtin`] defaults for common Linux apps;
/// [`Categories::from_toml_str`] / [`apply_toml_str`](Self::apply_toml_str)
/// merge user overrides on top (keys case-insensitive, unknown app_ids
/// fall back to `Other`).
#[derive(Debug, Clone)]
pub struct Categories {
    map: HashMap<String, Category>,
}

impl Categories {
    /// Builtin defaults for common Linux apps.
    pub fn builtin() -> Self {
        const SOCIAL: &[&str] = &[
            "firefox",
            "zen",
            "chromium",
            "google-chrome",
            "chrome",
            "brave",
            "vivaldi",
            "discord",
            "telegram-desktop",
            "telegram",
            "slack",
            "element",
            "fractal",
            "signal",
            "whatsapp",
            "teams",
        ];
        const PRODUCTIVITY: &[&str] = &[
            "code",
            "vscode",
            "vscodium",
            "cursor",
            "t3code",
            "t3-code",
            "windsurf",
            "zed",
            "neovim",
            "nvim",
            "vim",
            "emacs",
            "helix",
            "obsidian",
            "notion",
            "logseq",
            "joplin",
            "anki",
            "libreoffice-writer",
            "libreoffice-calc",
            "libreoffice-impress",
            "libreoffice",
            "soffice",
            "evince",
            "okular",
            "zathura",
            "thunderbird",
            "evolution",
            "geary",
            "gnucash",
            "kmymoney",
            "calibre",
            "xournalpp",
        ];
        const UTILITIES: &[&str] = &[
            "kitty",
            "alacritty",
            "foot",
            "gnome-terminal",
            "konsole",
            "wezterm",
            "xterm",
            "nautilus",
            "dolphin",
            "thunar",
            "nemo",
            "pcmanfm",
            "htop",
            "btop",
            "nvtop",
            "gnome-system-monitor",
            "settings",
            "gnome-settings",
            "pavucontrol",
            "blueman-manager",
            "nm-connection-editor",
            "file-roller",
            "loupe",
            "eog",
            "rofi",
            "wofi",
            "dunst",
            "mako",
            "waybar",
            "caelestia",
            "hyprland",
        ];
        const GAMING: &[&str] = &[
            "steam",
            "lutris",
            "heroic",
            "heroic-games-launcher",
            "bottles",
        ];
        const MEDIA: &[&str] = &[
            "vlc", "mpv", "obs", "obs-studio", "spotify",
        ];
        let mut map = HashMap::with_capacity(
            SOCIAL.len() + PRODUCTIVITY.len() + UTILITIES.len() + GAMING.len() + MEDIA.len(),
        );
        for a in SOCIAL {
            map.insert(a.to_string(), Category::Social);
        }
        for a in PRODUCTIVITY {
            map.insert(a.to_string(), Category::ProductivityFinance);
        }
        for a in UTILITIES {
            map.insert(a.to_string(), Category::Utilities);
        }
        for a in GAMING {
            map.insert(a.to_string(), Category::Gaming);
        }
        for a in MEDIA {
            map.insert(a.to_string(), Category::Media);
        }
        Self { map }
    }

    /// Builtins plus overrides from a TOML string (`[categories]` table;
    /// legacy `[apps]` table is merged first, `[categories]` wins).
    /// Unknown `[aliases]`/`[privacy]` keys are ignored.
    pub fn from_toml_str(s: &str) -> Result<Self> {
        let mut cats = Self::builtin();
        cats.apply_toml_str(s)?;
        Ok(cats)
    }

    /// Merge overrides from a TOML string into `self`.
    pub fn apply_toml_str(&mut self, s: &str) -> Result<()> {
        let parsed: CategoryToml =
            toml::from_str(s).context("parse categories TOML")?;
        // Legacy `[apps]` first so canonical `[categories]` wins on conflict.
        for (app_id, cat_name) in &parsed.apps {
            let cat =
                Category::parse(cat_name).with_context(|| {
                    format!("invalid category for app {app_id:?}")
                })?;
            self.map.insert(app_id.to_lowercase(), cat);
        }
        for (key, value) in &parsed.categories {
            match value {
                toml::Value::String(cat_name) => {
                    // Map style: app id -> category (strict, historical).
                    let cat = Category::parse(cat_name).with_context(|| {
                        format!("invalid category for app {key:?}")
                    })?;
                    self.map.insert(key.to_lowercase(), cat);
                }
                toml::Value::Array(_) | toml::Value::Table(_) => {
                    // Section/inline-list style: category -> apps.
                    // Unknown keys are ignored so a partial config never
                    // breaks history reads.
                    let Ok(cat) = Category::parse(key) else {
                        continue;
                    };
                    for app_id in category_apps(value) {
                        self.map.insert(app_id, cat);
                    }
                }
                _ => continue,
            }
        }
        Ok(())
    }

    pub fn insert(&mut self, app_id: &str, cat: Category) {
        self.map.insert(app_id.to_lowercase(), cat);
    }

    /// Builtins plus `~/.config/actwatch/categories.toml` overrides when the
    /// file exists (`XDG_CONFIG_HOME` honored). Never fails hard:
    /// unreadable/missing config yields builtins.
    pub fn load() -> Self {
        let mut cats = Self::builtin();
        if let Ok(home) = std::env::var("HOME") {
            if !home.is_empty() {
                for base in [
                    std::env::var("XDG_CONFIG_HOME").ok().filter(|d| !d.trim().is_empty()).map(std::path::PathBuf::from),
                    Some(std::path::PathBuf::from(home).join(".config")),
                ]
                .into_iter()
                .flatten()
                {
                    let path = base.join("actwatch/categories.toml");
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        let _ = cats.apply_toml_str(&content);
                        break;
                    }
                }
            }
        }
        cats
    }

    /// Resolve an app_id (case-insensitive) to its category. Falls back to
    /// the tail after the last dot, so reverse-DNS ids like
    /// `com.t3tools.T3Code` hit the `t3code` builtin.
    pub fn categorize(&self, app_id: &str) -> Category {
        let key = app_id.to_lowercase();
        if let Some(&cat) = self.map.get(&key) {
            return cat;
        }
        if let Some((_, tail)) = key.rsplit_once('.') {
            if let Some(&cat) = self.map.get(tail) {
                return cat;
            }
        }
        Category::Other
    }
}

/// App ids under a section/inline-list `[categories.<key>]` value: either a
/// table with an `apps` array or a bare string array. Anything else yields
/// nothing (a partial config must never break history reads).
fn category_apps(value: &toml::Value) -> Vec<String> {
    let items = match value {
        toml::Value::Array(items) => items.as_slice(),
        toml::Value::Table(table) => match table.get("apps") {
            Some(toml::Value::Array(items)) => items.as_slice(),
            _ => return Vec::new(),
        },
        _ => return Vec::new(),
    };
    items
        .iter()
        .filter_map(|v| v.as_str())
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_defaults_cover_common_apps() {
        let c = Categories::builtin();
        assert_eq!(c.categorize("firefox"), Category::Social);
        assert_eq!(c.categorize("discord"), Category::Social);
        assert_eq!(c.categorize("code"), Category::ProductivityFinance);
        assert_eq!(c.categorize("libreoffice-writer"), Category::ProductivityFinance);
        assert_eq!(c.categorize("kitty"), Category::Utilities);
        assert_eq!(c.categorize("nautilus"), Category::Utilities);
    }

    #[test]
    fn builtin_gaming_and_media_defaults() {
        let c = Categories::builtin();
        assert_eq!(c.categorize("steam"), Category::Gaming);
        assert_eq!(c.categorize("lutris"), Category::Gaming);
        assert_eq!(c.categorize("heroic"), Category::Gaming);
        assert_eq!(c.categorize("bottles"), Category::Gaming);
        assert_eq!(c.categorize("vlc"), Category::Media);
        assert_eq!(c.categorize("mpv"), Category::Media);
        assert_eq!(c.categorize("obs"), Category::Media);
        assert_eq!(c.categorize("spotify"), Category::Media);
        assert_eq!(
            c.categorize("definitely-not-an-app"),
            Category::Other
        );
    }

    #[test]
    fn categorize_is_case_insensitive() {
        let c = Categories::builtin();
        assert_eq!(c.categorize("Firefox"), Category::Social);
        assert_eq!(c.categorize("KITTY"), Category::Utilities);
    }

    #[test]
    fn toml_overrides_merge_over_builtins() {
        let c = Categories::from_toml_str(
            "[apps]\nfirefox = \"Utilities\"\nsteam = \"Social\"\n",
        )
        .unwrap();
        assert_eq!(c.categorize("firefox"), Category::Utilities);
        assert_eq!(c.categorize("steam"), Category::Social);
        // Untouched builtins survive.
        assert_eq!(c.categorize("kitty"), Category::Utilities);
    }

    #[test]
    fn categories_table_is_canonical_and_wins_over_legacy_apps() {
        let c = Categories::from_toml_str(
            "[apps]\nvlc = \"Social\"\n[categories]\nvlc = \"Media\"\nmpv = \"Gaming\"\n",
        )
        .unwrap();
        assert_eq!(c.categorize("vlc"), Category::Media);
        assert_eq!(c.categorize("mpv"), Category::Gaming);
        // Unrelated config keys are ignored.
        let c = Categories::from_toml_str(
            "[aliases]\nfoo = \"Foo\"\n[privacy]\ntitles = \"full\"\n[categories]\nfoo = \"Social\"\n",
        )
        .unwrap();
        assert_eq!(c.categorize("foo"), Category::Social);
    }

    #[test]
    fn section_and_inline_list_styles_work() {
        let c = Categories::from_toml_str(
            "[categories.gaming]\napps = [\"Steam\", \"lutris\"]\n\
             [categories]\nmedia = [\"VLC\"]\nfirefox = \"Social\"\n",
        )
        .unwrap();
        assert_eq!(c.categorize("steam"), Category::Gaming);
        assert_eq!(c.categorize("lutris"), Category::Gaming);
        assert_eq!(c.categorize("vlc"), Category::Media);
        assert_eq!(c.categorize("firefox"), Category::Social);
        // Unknown section keys are ignored, never fatal.
        let c = Categories::from_toml_str(
            "[categories.bogus]\napps = [\"x\"]\n",
        )
        .unwrap();
        assert_eq!(c.categorize("x"), Category::Other);
        // Config-file key spellings parse.
        assert_eq!(
            Category::parse("productivity_finance").unwrap(),
            Category::ProductivityFinance
        );
    }

    #[test]
    fn category_parse_accepts_aliases() {        assert_eq!(Category::parse("social").unwrap(), Category::Social);
        assert_eq!(
            Category::parse("Productivity").unwrap(),
            Category::ProductivityFinance
        );
        assert_eq!(Category::parse("utility").unwrap(), Category::Utilities);
        assert_eq!(Category::parse("Gaming").unwrap(), Category::Gaming);
        assert_eq!(Category::parse("games").unwrap(), Category::Gaming);
        assert_eq!(Category::parse("Media").unwrap(), Category::Media);
        assert_eq!(Category::parse("multimedia").unwrap(), Category::Media);
        assert!(Category::parse("Arcade").is_err());
    }
}
