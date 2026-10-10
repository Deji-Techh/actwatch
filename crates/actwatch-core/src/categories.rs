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
            | "finance" => Ok(Category::ProductivityFinance),
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
    /// Canonical override table (v2 contract).
    #[serde(default)]
    categories: HashMap<String, String>,
    /// Legacy alias for `[categories]` (pre-v2 config files).
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
        for (app_id, cat_name) in parsed.apps.iter().chain(parsed.categories.iter()) {
            let cat =
                Category::parse(cat_name).with_context(|| {
                    format!("invalid category for app {app_id:?}")
                })?;
            self.map.insert(app_id.to_lowercase(), cat);
        }
        Ok(())
    }

    pub fn insert(&mut self, app_id: &str, cat: Category) {
        self.map.insert(app_id.to_lowercase(), cat);
    }

    /// Resolve an app_id (case-insensitive) to its category.
    pub fn categorize(&self, app_id: &str) -> Category {
        self.map
            .get(&app_id.to_lowercase())
            .copied()
            .unwrap_or(Category::Other)
    }
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
    fn category_parse_accepts_aliases() {
        assert_eq!(Category::parse("social").unwrap(), Category::Social);
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
