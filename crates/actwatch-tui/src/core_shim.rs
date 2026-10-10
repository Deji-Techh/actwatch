//! v2 Core contract shims: 6 Categories and DisplayNames.
//!
//! When actwatch-core v2 lands with 6 categories and DisplayNames, this module
//! can be replaced by direct re-exports from actwatch-core.

use anyhow::{bail, Context, Result};
use ratatui::style::Color;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// The 6 canonical dashboard categories per the frozen v2 contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
pub enum Category {
    Social,
    ProductivityFinance,
    Utilities,
    Gaming,
    Media,
    Other,
}

impl Category {
    /// All 6 categories in legend order.
    pub const ALL: [Category; 6] = [
        Category::Social,
        Category::ProductivityFinance,
        Category::Utilities,
        Category::Gaming,
        Category::Media,
        Category::Other,
    ];

    pub fn name(&self) -> &'static str {
        match self {
            Category::Social => "Social",
            Category::ProductivityFinance => "Productivity & Finance",
            Category::Utilities => "Utilities",
            Category::Gaming => "Gaming",
            Category::Media => "Media",
            Category::Other => "Other",
        }
    }

    pub fn short_name(&self) -> &'static str {
        match self {
            Category::Social => "Social",
            Category::ProductivityFinance => "Productivity",
            Category::Utilities => "Utilities",
            Category::Gaming => "Gaming",
            Category::Media => "Media",
            Category::Other => "Other",
        }
    }

    /// Distinct monochrome fill pattern character per category.
    pub fn fill_char(&self) -> char {
        match self {
            Category::Social => '█',              // Solid full block
            Category::ProductivityFinance => '▓', // Dark shade / dense dither
            Category::Utilities => '▒',           // Medium shade dither
            Category::Gaming => '░',              // Light shade dither
            Category::Media => '▚',               // Checkerboard dither
            Category::Other => '▤',               // Horizontal hatch
        }
    }

    /// Dedicated gray shade for monochrome zero-hue palette.
    pub fn gray_shade(&self) -> Color {
        match self {
            Category::Social => Color::Rgb(212, 212, 216),              // #D4D4D8
            Category::ProductivityFinance => Color::Rgb(161, 161, 170), // #A1A1AA
            Category::Utilities => Color::Rgb(113, 113, 122),           // #71717A
            Category::Gaming => Color::Rgb(228, 228, 231),              // #E4E4E7
            Category::Media => Color::Rgb(82, 82, 91),                  // #52525B
            Category::Other => Color::Rgb(63, 63, 70),                  // #3F3F46
        }
    }

    /// Parse category name with aliases.
    pub fn parse(s: &str) -> Result<Self> {
        match s.trim().to_lowercase().as_str() {
            "social" => Ok(Category::Social),
            "productivity & finance" | "productivity and finance" | "productivity"
            | "finance" => Ok(Category::ProductivityFinance),
            "utilities" | "utility" => Ok(Category::Utilities),
            "gaming" | "games" | "game" => Ok(Category::Gaming),
            "media" | "audio" | "video" | "multimedia" => Ok(Category::Media),
            "other" | "misc" | "miscellaneous" | "uncategorized" => Ok(Category::Other),
            _ => bail!("unknown category: {s:?}"),
        }
    }
}

impl std::fmt::Display for Category {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

#[derive(Debug, Deserialize, Default)]
struct ConfigToml {
    #[serde(default)]
    categories: HashMap<String, String>,
    #[serde(default)]
    aliases: HashMap<String, String>,
}

/// Resolves an `app_id` to one of the 6 canonical categories.
#[derive(Debug, Clone)]
pub struct Categories {
    map: HashMap<String, Category>,
}

impl Default for Categories {
    fn default() -> Self {
        Self::load()
    }
}

impl Categories {
    /// Builtin mappings per the v2 contract.
    pub fn builtin() -> Self {
        let mut map = HashMap::new();

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
            "zoom",
            "webcord",
            "vesktop",
            "skype",
            "betterdiscord",
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
            "postman",
            "dbeaver",
            "intellij",
            "clion",
            "pycharm",
            "goland",
            "rider",
            "rustrover",
        ];

        const UTILITIES: &[&str] = &[
            "kitty",
            "alacritty",
            "foot",
            "gnome-terminal",
            "konsole",
            "wezterm",
            "xterm",
            "ghostty",
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
            "calculator",
            "wlogout",
        ];

        const GAMING: &[&str] = &[
            "steam",
            "lutris",
            "heroic",
            "bottles",
            "minecraft",
            "heroicgameslauncher",
            "prism-launcher",
            "retroarch",
            "mindustry",
        ];

        const MEDIA: &[&str] = &[
            "vlc",
            "mpv",
            "obs",
            "spotify",
            "gimp",
            "blender",
            "inkscape",
            "audacity",
            "kdenlive",
            "rhythmbox",
            "deadbeef",
            "plex",
            "jellyfin",
        ];

        for a in SOCIAL {
            map.insert(a.to_lowercase(), Category::Social);
        }
        for a in PRODUCTIVITY {
            map.insert(a.to_lowercase(), Category::ProductivityFinance);
        }
        for a in UTILITIES {
            map.insert(a.to_lowercase(), Category::Utilities);
        }
        for a in GAMING {
            map.insert(a.to_lowercase(), Category::Gaming);
        }
        for a in MEDIA {
            map.insert(a.to_lowercase(), Category::Media);
        }

        Self { map }
    }

    /// Load builtins and merge config file if present.
    pub fn load() -> Self {
        let mut cats = Self::builtin();
        if let Some(config_path) = config_file_path() {
            if let Ok(content) = fs::read_to_string(config_path) {
                let _ = cats.apply_toml_str(&content);
            }
        }
        cats
    }

    pub fn apply_toml_str(&mut self, s: &str) -> Result<()> {
        let parsed: ConfigToml = toml::from_str(s).context("parse categories TOML")?;
        for (app_id, cat_name) in &parsed.categories {
            if let Ok(cat) = Category::parse(cat_name) {
                self.map.insert(app_id.to_lowercase(), cat);
            }
        }
        Ok(())
    }

    pub fn categorize(&self, app_id: &str) -> Category {
        let clean = app_id.trim().to_lowercase();
        if let Some(&cat) = self.map.get(&clean) {
            return cat;
        }

        // Check if stem or reverse-DNS suffix matches
        let stem = clean.rsplit('.').next().unwrap_or(&clean);
        if let Some(&cat) = self.map.get(stem) {
            return cat;
        }

        Category::Other
    }
}

/// Friendly Display Names resolver for Linux applications.
#[derive(Debug, Clone)]
pub struct DisplayNames {
    desktop_names: HashMap<String, String>,
    aliases: HashMap<String, String>,
}

impl Default for DisplayNames {
    fn default() -> Self {
        Self::load()
    }
}

impl DisplayNames {
    /// Load desktop entry mappings and user aliases.
    pub fn load() -> Self {
        let mut desktop_names = HashMap::new();
        let mut aliases = HashMap::new();

        // Scan standard desktop application directories
        let search_dirs = [
            PathBuf::from("/usr/share/applications"),
            PathBuf::from("/usr/local/share/applications"),
            dirs_local_applications(),
        ];

        for dir in &search_dirs {
            if dir.is_dir() {
                scan_desktop_dir(dir, &mut desktop_names);
            }
        }

        // Read user aliases from ~/.config/actwatch/categories.toml
        if let Some(config_path) = config_file_path() {
            if let Ok(content) = fs::read_to_string(config_path) {
                if let Ok(parsed) = toml::from_str::<ConfigToml>(&content) {
                    for (k, v) in parsed.aliases {
                        aliases.insert(k.to_lowercase(), v);
                    }
                }
            }
        }

        Self {
            desktop_names,
            aliases,
        }
    }

    /// Resolve an app_id to a human-friendly display name.
    pub fn resolve(&self, app_id: &str) -> String {
        let lower = app_id.trim().to_lowercase();

        // 1. Reserved away / idle identifiers
        if lower == "away" || lower == "idle" || lower == "lockscreen" {
            return "Away".to_string();
        }

        // 2. Configured user aliases
        if let Some(alias) = self.aliases.get(&lower) {
            return alias.clone();
        }

        // 3. Match from desktop entries
        if let Some(name) = self.desktop_names.get(&lower) {
            return name.clone();
        }

        let stem = lower.rsplit('.').next().unwrap_or(&lower);
        if let Some(name) = self.desktop_names.get(stem) {
            return name.clone();
        }

        // 4. Prettify fallback
        prettify_app_id(app_id)
    }
}

fn dirs_local_applications() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("applications")
    } else {
        PathBuf::from(".local/share/applications")
    }
}

fn config_file_path() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    let path = PathBuf::from(home)
        .join(".config")
        .join("actwatch")
        .join("categories.toml");
    Some(path)
}

fn scan_desktop_dir(dir: &Path, map: &mut HashMap<String, String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("desktop") {
            if let Ok(content) = fs::read_to_string(&path) {
                parse_desktop_file(&path, &content, map);
            }
        }
    }
}

fn parse_desktop_file(path: &Path, content: &str, map: &mut HashMap<String, String>) {
    let mut name = None;
    let mut wm_class = None;
    let mut in_desktop_entry = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "[Desktop Entry]" {
            in_desktop_entry = true;
            continue;
        } else if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_desktop_entry = false;
        }

        if in_desktop_entry {
            if let Some(val) = trimmed.strip_prefix("Name=") {
                if name.is_none() {
                    name = Some(val.trim().to_string());
                }
            } else if let Some(val) = trimmed.strip_prefix("StartupWMClass=") {
                if wm_class.is_none() {
                    wm_class = Some(val.trim().to_string());
                }
            }
        }
    }

    if let Some(app_name) = name {
        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
            map.entry(stem.to_lowercase()).or_insert_with(|| app_name.clone());
        }
        if let Some(wm) = wm_class {
            map.entry(wm.to_lowercase()).or_insert(app_name);
        }
    }
}

/// Prettifies an app_id: strips reverse-DNS prefix, replaces punctuation with spaces, Title-Cases words.
pub fn prettify_app_id(app_id: &str) -> String {
    let raw = app_id.trim();
    if raw.is_empty() {
        return String::new();
    }

    // Strip reverse-DNS prefix like com.t3tools.T3Code -> T3Code, org.wezfurlong.wezterm -> wezterm
    let stripped = if raw.starts_with("com.")
        || raw.starts_with("org.")
        || raw.starts_with("io.")
        || raw.starts_with("net.")
        || raw.starts_with("app.")
        || raw.starts_with("de.")
        || raw.starts_with("fr.")
    {
        raw.rsplit('.').next().unwrap_or(raw)
    } else {
        raw
    };

    // Replace '-', '_', '.' with spaces
    let with_spaces = stripped.replace(['-', '_', '.'], " ");

    // Title case words
    with_spaces
        .split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => {
                    let mut s = String::new();
                    s.extend(first.to_uppercase());
                    s.push_str(chars.as_str());
                    s
                }
            }
        })
        .collect::<Vec<String>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prettify_app_id() {
        assert_eq!(prettify_app_id("com.t3tools.T3Code"), "T3Code");
        assert_eq!(prettify_app_id("google-chrome"), "Google Chrome");
        assert_eq!(prettify_app_id("foot"), "Foot");
        assert_eq!(prettify_app_id("libreoffice-writer"), "Libreoffice Writer");
        assert_eq!(prettify_app_id("org.wezfurlong.wezterm"), "Wezterm");
    }

    #[test]
    fn test_v2_categories() {
        let cats = Categories::builtin();
        assert_eq!(cats.categorize("firefox"), Category::Social);
        assert_eq!(cats.categorize("discord"), Category::Social);
        assert_eq!(cats.categorize("code"), Category::ProductivityFinance);
        assert_eq!(cats.categorize("foot"), Category::Utilities);
        assert_eq!(cats.categorize("kitty"), Category::Utilities);
        assert_eq!(cats.categorize("steam"), Category::Gaming);
        assert_eq!(cats.categorize("vlc"), Category::Media);
        assert_eq!(cats.categorize("unknown_app"), Category::Other);
    }

    #[test]
    fn test_display_names_away_resolution() {
        let names = DisplayNames {
            desktop_names: HashMap::new(),
            aliases: HashMap::new(),
        };
        assert_eq!(names.resolve("away"), "Away");
        assert_eq!(names.resolve("idle"), "Away");
        assert_eq!(names.resolve("com.t3tools.T3Code"), "T3Code");
    }
}
