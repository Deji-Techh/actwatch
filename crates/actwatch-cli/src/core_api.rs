//! The single seam between the CLI and `actwatch-core`.
//!
//! v2 contract (docs/ARCHITECTURE.md, "v2 contract (FROZEN)"): display names,
//! categories, the title policy and the reserved `"away"` id all come from
//! `actwatch-core`, so every frontend agrees and the CLI owns no maps of its
//! own. Only [`Catalog`] touches the core types; renderers take an `&Catalog`
//! and never re-derive a name or a category.
//!
//! `actwatch-core` on this branch still ships the v1 surface (four categories,
//! no `names` module, no `TitlePolicy`). The pieces the v2 contract requires
//! but core does not expose yet live in [`shim`], and every one of them is
//! marked `SHIM:` below. When core v2 merges, delete the `SHIM:` items and
//! re-point the `pub use` lines — no call site below this module changes.
//! See docs/ARCHITECTURE.md, "CLI v2 status", for the full swap list.

use std::collections::HashMap;
use std::path::PathBuf;

pub use shim::{Category, DisplayNames, TitlePolicy};

/// Reserved app id for idle / lock spans (v2 contract, "Away + permissions").
/// Frontends keep it out of app totals and render it as its own row.
pub const AWAY_APP_ID: &str = "away";

/// Label for [`AWAY_APP_ID`] in every renderer.
pub const AWAY_LABEL: &str = "Away";

/// Directories scanned for `.desktop` entries by [`DisplayNames`].
pub const DESKTOP_DIRS: [&str; 2] =
    ["/usr/share/applications", ".local/share/applications"];

/// User config, per the v2 contract: `~/.config/actwatch/categories.toml`.
pub fn config_path() -> PathBuf {
    crate::db::config_path()
}

/// Everything the CLI needs to turn a raw `app_id` into something a person
/// wants to read. Cheap to build once per process and immutable afterwards.
#[derive(Debug, Clone)]
pub struct Catalog {
    names: DisplayNames,
    categories: shim::Categories,
    policy: TitlePolicy,
}

impl Catalog {
    /// Builds the catalog for this machine. Never fails: a missing, unreadable
    /// or malformed config falls back to the builtins, because a broken config
    /// must not stop someone from reading their own history.
    pub fn load() -> Self {
        let path = config_path();
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let mut catalog = Self::from_toml_str(&text);
        catalog.names.load_desktop_files();
        catalog
    }

    /// Builds a catalog from TOML text plus the system `.desktop` entries.
    /// Used by [`Catalog::load`] and by the tests.
    pub fn from_toml_str(text: &str) -> Self {
        let cfg = Config::parse(text);
        Self {
            names: DisplayNames::new(cfg.aliases),
            categories: shim::Categories::with_overrides(cfg.category_overrides),
            policy: cfg.titles,
        }
    }

    /// Resolves a raw app id to its label and category.
    pub fn lookup(&self, app_id: &str) -> AppInfo {
        AppInfo {
            app_id: app_id.to_string(),
            name: self.names.resolve(app_id),
            category: self.categories.categorize(app_id),
        }
    }

    /// True for the reserved idle/lock app id.
    pub fn is_away(app_id: &str) -> bool {
        app_id.eq_ignore_ascii_case(AWAY_APP_ID)
    }

    /// Configured window-title policy. The CLI is app-level only and never
    /// renders titles; this exists so `status` can tell the truth about what
    /// the daemon recorded.
    pub fn title_policy(&self) -> TitlePolicy {
        self.policy
    }

    #[allow(dead_code)] // inspection accessor for tests
    pub fn categories(&self) -> &shim::Categories {
        &self.categories
    }

    #[allow(dead_code)] // inspection accessor for tests
    pub fn names(&self) -> &DisplayNames {
        &self.names
    }
}

/// A resolved app: raw id, friendly name and category.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppInfo {
    pub app_id: String,
    pub name: String,
    pub category: Category,
}

impl AppInfo {
    pub fn is_away(&self) -> bool {
        Catalog::is_away(&self.app_id)
    }

    /// Category label, or `None` for away rows (away is not a category).
    pub fn category_label(&self) -> Option<&'static str> {
        if self.is_away() {
            None
        } else {
            Some(self.category.as_str())
        }
    }
}

// ---------------------------------------------------------------------------
// Config file
// ---------------------------------------------------------------------------

/// Parsed `~/.config/actwatch/categories.toml`.
///
/// Recognised keys (all optional, all case-insensitive for app ids):
///
/// ```toml
/// [privacy]
/// titles = "off"            # off | hash | full
///
/// [aliases]
/// t3code = "T3 Code"        # app id -> friendly name
///
/// [categories.social]
/// apps = ["firefox"]        # app id -> category
/// ```
///
/// `[categories]` also accepts the list shorthand (`social = ["firefox"]`),
/// and the v1 flat `[apps]` table (`firefox = "Social"`) is still honoured so
/// an existing config keeps working. Unknown keys and categories are ignored
/// rather than fatal.
#[derive(Debug, Default)]
struct Config {
    aliases: HashMap<String, String>,
    category_overrides: HashMap<String, Category>,
    titles: TitlePolicy,
}

impl Config {
    fn parse(text: &str) -> Self {
        let mut cfg = Config::default();
        if text.trim().is_empty() {
            return cfg;
        }
        // A malformed config must not make the CLI unusable.
        let Ok(table) = text.parse::<toml::Table>() else {
            return cfg;
        };

        if let Some(aliases) = table.get("aliases").and_then(toml::Value::as_table) {
            // TOML reads `org.gnome.Nautilus = "Files"` as nested tables, so
            // both the quoted and the bare spelling have to work.
            let mut entries = Vec::new();
            flat_entries(aliases, "", &mut entries);
            for (app_id, value) in entries {
                if let Some(name) = value.as_str() {
                    cfg.aliases.insert(app_id.to_lowercase(), name.trim().to_string());
                }
            }
        }

        if let Some(categories) = table.get("categories").and_then(toml::Value::as_table) {
            for (key, value) in categories {
                let Some(category) = Category::parse_key(key) else {
                    continue;
                };
                for app_id in app_id_list(value) {
                    cfg.category_overrides
                        .insert(app_id.to_lowercase(), category);
                }
            }
        }

        // v1 flat table: app_id = "Category".
        if let Some(apps) = table.get("apps").and_then(toml::Value::as_table) {
            for (app_id, value) in apps {
                let Some(name) = value.as_str() else {
                    continue;
                };
                let Some(category) = Category::parse(name) else {
                    continue;
                };
                cfg.category_overrides
                    .insert(app_id.to_lowercase(), category);
            }
        }

        cfg.titles = table
            .get("privacy")
            .and_then(toml::Value::as_table)
            .and_then(|p| p.get("titles"))
            .and_then(toml::Value::as_str)
            .and_then(TitlePolicy::parse)
            .unwrap_or_default();

        cfg
    }
}

/// Flattens `a.b.c = value` written as nested tables back into dotted keys.
fn flat_entries(table: &toml::Table, prefix: &str, out: &mut Vec<(String, toml::Value)>) {
    for (key, value) in table {
        let path = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        match value {
            toml::Value::Table(inner) => flat_entries(inner, &path, out),
            leaf => out.push((path, leaf.clone())),
        }
    }
}

/// App ids under a `[categories.<key>]` entry: either a table with an `apps`
/// array, or the bare list shorthand.
fn app_id_list(value: &toml::Value) -> Vec<String> {
    let items = match value {
        toml::Value::Array(items) => items,
        toml::Value::Table(table) => match table.get("apps") {
            Some(toml::Value::Array(items)) => items,
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

// ---------------------------------------------------------------------------
// Temporary stand-ins for the v2 contract APIs missing from actwatch-core.
// Delete this module when core v2 merges; the `pub use` at the top of the file
// is the only thing that points somewhere.
// ---------------------------------------------------------------------------

// Contract surface: a few items exist to match the v2 core API one-for-one and
// are only exercised by the tests here. They disappear with this module.
#[allow(dead_code)]
pub mod shim {
    //! Contract behaviour that `actwatch-core` v1 does not expose yet.
    //!
    //! Every item here is a placeholder for a named v2 core API, never a
    //! CLI-local opinion:
    //!
    //! | shim item        | v2 core API it stands in for                |
    //! |------------------|--------------------------------------------|
    //! | `DisplayNames`   | `core::names::DisplayNames`                 |
    //! | `Categories`     | `core::Categories` (six variants)           |
    //! | `TitlePolicy`    | `core::TitlePolicy`                         |
    //!
    //! `Categories` deliberately delegates: anything `actwatch-core` already
    //! classifies wins, and only the two buckets core v1 cannot express
    //! (Gaming, Media) are filled in here.

    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    /// SHIM: v2 `core::Category` (adds Gaming + Media to the v1 four).
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
        /// Legend order, matching the dashboard.
        pub const ALL: [Category; 6] = [
            Category::Social,
            Category::ProductivityFinance,
            Category::Utilities,
            Category::Gaming,
            Category::Media,
            Category::Other,
        ];

        /// Display label used by every frontend.
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

        /// TOML table key under `[categories]`.
        pub fn key(self) -> &'static str {
            match self {
                Category::Social => "social",
                Category::ProductivityFinance => "productivity_finance",
                Category::Utilities => "utilities",
                Category::Gaming => "gaming",
                Category::Media => "media",
                Category::Other => "other",
            }
        }

        /// Parses a display label, tolerating the v1 names and `&`/`and`
        /// spelling differences.
        pub fn parse(s: &str) -> Option<Self> {
            match s.trim().to_lowercase().as_str() {
                "social" => Some(Category::Social),
                "productivity & finance"
                | "productivity and finance"
                | "productivity_finance"
                | "productivity &finance"
                | "productivity"
                | "finance" => Some(Category::ProductivityFinance),
                "utilities" | "utility" | "terminal & tiling" | "terminal" => {
                    Some(Category::Utilities)
                }
                "gaming" | "games" => Some(Category::Gaming),
                "media" | "multimedia" => Some(Category::Media),
                "other" | "misc" | "miscellaneous" | "uncategorized" => {
                    Some(Category::Other)
                }
                _ => None,
            }
        }

        /// Parses a `[categories.<key>]` table name.
        pub fn parse_key(key: &str) -> Option<Self> {
            Self::parse(key)
        }
    }

    impl std::fmt::Display for Category {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(self.as_str())
        }
    }

    /// SHIM: v2 `core::Categories`.
    ///
    /// Resolution order follows the contract: `[categories]` overrides first,
    /// then the builtins. The builtins come from `actwatch-core` itself;
    /// Gaming and Media are the only lists written out here, because core v1
    /// folds both into `Other`.
    #[derive(Debug, Clone)]
    pub struct Categories {
        core: actwatch_core::Categories,
        overrides: HashMap<String, Category>,
    }

    impl Categories {
        /// The two categories `actwatch-core` v1 cannot represent.
        const GAMING: &'static [&'static str] = &[
            "steam", "steamwebhelper", "lutris", "heroic", "bottles", "gamescope",
            "minetest", "0x0at0yspace", "dolphin-emu", "pcsx2", "rpcs3", "citra",
            "yuzu", "ryujinx", "retroarch", "wine", "proton", "legendary",
        ];
        const MEDIA: &'static [&'static str] = &[
            "vlc", "mpv", "obs", "spotify", "mpris", "celluloid", "haruna",
            "smplayer", "mpvpaper", "gamescope-vsync", "strawberry", "audacious",
        ];

        pub fn builtin() -> Self {
            Self {
                core: actwatch_core::Categories::builtin(),
                overrides: HashMap::new(),
            }
        }

        pub fn with_overrides(overrides: HashMap<String, Category>) -> Self {
            Self {
                core: actwatch_core::Categories::builtin(),
                overrides,
            }
        }

        pub fn categorize(&self, app_id: &str) -> Category {
            let key = app_id.to_lowercase();
            if let Some(cat) = self.overrides.get(&key) {
                return *cat;
            }
            // actwatch-core owns the four v1 categories.
            match self.core.categorize(app_id) {
                actwatch_core::Category::Social => Category::Social,
                actwatch_core::Category::ProductivityFinance => {
                    Category::ProductivityFinance
                }
                actwatch_core::Category::Utilities => Category::Utilities,
                actwatch_core::Category::Other => {
                    if Self::GAMING.contains(&key.as_str()) {
                        Category::Gaming
                    } else if Self::MEDIA.contains(&key.as_str()) {
                        Category::Media
                    } else {
                        Category::Other
                    }
                }
            }
        }
    }

    /// SHIM: v2 `core::TitlePolicy`.
    ///
    /// A read-only frontend only needs to know the policy so `status` can
    /// report it; `redact` is applied by the daemon at write time and is not
    /// part of the CLI's seam.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub enum TitlePolicy {
        #[default]
        Off,
        Hash,
        Full,
    }

    impl TitlePolicy {
        pub fn parse(s: &str) -> Option<Self> {
            match s.trim().to_lowercase().as_str() {
                "off" | "none" | "false" => Some(TitlePolicy::Off),
                "hash" | "hashed" => Some(TitlePolicy::Hash),
                "full" | "true" => Some(TitlePolicy::Full),
                _ => None,
            }
        }

        pub fn as_str(self) -> &'static str {
            match self {
                TitlePolicy::Off => "off",
                TitlePolicy::Hash => "hash",
                TitlePolicy::Full => "full",
            }
        }

        /// One-line explanation for `status`.
        pub fn description(self) -> &'static str {
            match self {
                TitlePolicy::Off => "window titles are not stored",
                TitlePolicy::Hash => "window titles are stored as a hash",
                TitlePolicy::Full => "window titles are stored verbatim",
            }
        }
    }

    /// SHIM: v2 `core::names::DisplayNames`.
    ///
    /// `resolve()` order: `[aliases]` override, then a `.desktop` match
    /// (`StartupWMClass`, else the file stem), then a prettified app id.
    #[derive(Debug, Clone, Default)]
    pub struct DisplayNames {
        aliases: HashMap<String, String>,
        desktop: HashMap<String, String>,
    }

    impl DisplayNames {
        pub fn new(aliases: HashMap<String, String>) -> Self {
            Self {
                aliases,
                desktop: HashMap::new(),
            }
        }

        /// Scans `/usr/share/applications` and
        /// `~/.local/share/applications` for `.desktop` entries.
        pub fn load_desktop_files(&mut self) {
            let mut dirs: Vec<PathBuf> = Vec::with_capacity(2);
            dirs.push(PathBuf::from(super::DESKTOP_DIRS[0]));
            dirs.push(
                crate::db::home_dir().join(super::DESKTOP_DIRS[1]),
            );
            for dir in dirs {
                self.scan_dir(&dir);
            }
        }

        /// Visible for tests: parse one `.desktop` file's contents.
        pub fn insert_desktop_entry(&mut self, file_stem: &str, contents: &str) {
            let mut name = None;
            let mut wm_class = None;
            for line in contents.lines() {
                let line = line.trim();
                if let Some(value) = line.strip_prefix("Name=") {
                    name = name.or_else(|| Some(value.trim().to_string()));
                } else if let Some(value) = line.strip_prefix("StartupWMClass=") {
                    wm_class = wm_class.or_else(|| Some(value.trim().to_string()));
                }
            }
            let Some(name) = name else {
                return;
            };
            self.desktop
                .insert(file_stem.to_lowercase(), name.clone());
            if let Some(wm_class) = wm_class {
                self.desktop.insert(wm_class.to_lowercase(), name);
            }
        }

        fn scan_dir(&mut self, dir: &Path) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
                    continue;
                }
                let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                    continue;
                };
                let Ok(contents) = std::fs::read_to_string(&path) else {
                    continue;
                };
                self.insert_desktop_entry(stem, &contents);
            }
        }

        /// Friendly label for an app id.
        pub fn resolve(&self, app_id: &str) -> String {
            if app_id.eq_ignore_ascii_case(super::AWAY_APP_ID) {
                return super::AWAY_LABEL.to_string();
            }
            let key = app_id.to_lowercase();
            if let Some(name) = self.aliases.get(&key) {
                return name.clone();
            }
            if let Some(name) = self.desktop.get(&key) {
                return name.clone();
            }
            // Reverse-DNS ids are usually filed as `t3code.desktop`.
            if let Some((_, tail)) = key.rsplit_once('.') {
                if let Some(name) = self.desktop.get(tail) {
                    return name.clone();
                }
            }
            prettify(app_id)
        }
    }

    /// Strips a reverse-DNS prefix, turns `-_.` into spaces and title-cases
    /// each word. Only the first letter of a word is touched, so brands like
    /// `T3Code` survive intact.
    fn prettify(app_id: &str) -> String {
        let trimmed = app_id.trim();
        if trimmed.is_empty() {
            return super::AWAY_LABEL.to_string();
        }
        let tail = match trimmed.rsplit_once('.') {
            Some((head, tail)) if !head.is_empty() && !tail.is_empty() => tail,
            _ => trimmed,
        };
        tail.split(['-', '_', '.', ' '])
            .filter(|word| !word.is_empty())
            .map(title_case)
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn title_case(word: &str) -> String {
        let mut chars = word.chars();
        match chars.next() {
            None => String::new(),
            Some(first) => {
                first.to_uppercase().collect::<String>() + chars.as_str()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog() -> Catalog {
        Catalog::from_toml_str("")
    }

    #[test]
    fn away_resolves_to_away_label() {
        let c = catalog();
        assert_eq!(c.lookup("away").name, "Away");
        assert_eq!(c.lookup("AWAY").name, "Away");
        assert!(Catalog::is_away("away"));
        assert!(!Catalog::is_away("firefox"));
        assert_eq!(c.lookup("away").category_label(), None);
    }

    #[test]
    fn prettifies_raw_app_ids() {
        let names = DisplayNames::default();
        assert_eq!(names.resolve("com.t3tools.T3Code"), "T3Code");
        assert_eq!(names.resolve("t3code"), "T3code");
        assert_eq!(names.resolve("google-chrome"), "Google Chrome");
        assert_eq!(names.resolve("org.gnome.Nautilus"), "Nautilus");
        assert_eq!(names.resolve("bitwarden"), "Bitwarden");
    }

    #[test]
    fn desktop_entry_wins_over_prettifying() {
        let mut names = DisplayNames::default();
        names.insert_desktop_entry(
            "t3code",
            "[Desktop Entry]\nName=T3 Code\nStartupWMClass=T3Code\nExec=t3code\n",
        );
        assert_eq!(names.resolve("T3Code"), "T3 Code");
        // Bare stem also matches a reverse-DNS id without a WMClass.
        names.insert_desktop_entry("customapp", "[Desktop Entry]\nName=Custom App\n");
        assert_eq!(names.resolve("com.vendor.customapp"), "Custom App");
    }

    #[test]
    fn dotted_app_ids_resolve_unquoted_or_quoted() {
        // Bare `org.gnome.Nautilus = "Files"` parses as nested tables in TOML;
        // the quoted spelling is literal. Both must land on the same alias.
        let bare = Catalog::from_toml_str("[aliases]\norg.gnome.Nautilus = \"Files\"\n");
        let quoted =
            Catalog::from_toml_str("[aliases]\n\"org.gnome.Nautilus\" = \"Files\"\n");
        assert_eq!(bare.lookup("org.gnome.Nautilus").name, "Files");
        assert_eq!(quoted.lookup("org.gnome.Nautilus").name, "Files");
        // Case-insensitive, like every other app id.
        assert_eq!(bare.lookup("Org.Gnome.Nautilus").name, "Files");
    }

    #[test]
    fn aliases_win_over_desktop_entries() {
        let mut catalog = Catalog::from_toml_str("[aliases]\nt3code = \"The Code\"\n");
        catalog.names.insert_desktop_entry(
            "t3code",
            "[Desktop Entry]\nName=T3 Code\n",
        );
        assert_eq!(catalog.lookup("t3code").name, "The Code");
    }

    #[test]
    fn six_categories_are_reachable() {
        let c = catalog();
        assert_eq!(c.categories().categorize("firefox"), Category::Social);
        assert_eq!(
            c.categories().categorize("code"),
            Category::ProductivityFinance
        );
        assert_eq!(c.categories().categorize("kitty"), Category::Utilities);
        assert_eq!(c.categories().categorize("steam"), Category::Gaming);
        assert_eq!(c.categories().categorize("lutris"), Category::Gaming);
        assert_eq!(c.categories().categorize("mpv"), Category::Media);
        assert_eq!(c.categories().categorize("spotify"), Category::Media);
        assert_eq!(
            c.categories().categorize("some-unknown-app"),
            Category::Other
        );
        assert_eq!(Category::ALL.len(), 6);
        let labels: Vec<&str> = Category::ALL.iter().map(|c| c.as_str()).collect();
        assert_eq!(
            labels,
            vec![
                "Social",
                "Productivity & Finance",
                "Utilities",
                "Gaming",
                "Media",
                "Other"
            ]
        );
    }

    #[test]
    fn category_overrides_beat_builtins() {
        let c = Catalog::from_toml_str(
            "[categories.gaming]\napps = [\"firefox\"]\n[categories]\nmedia = [\"code\"]\n",
        );
        assert_eq!(c.categories().categorize("firefox"), Category::Gaming);
        assert_eq!(c.categories().categorize("code"), Category::Media);
        // Untouched builtins survive.
        assert_eq!(c.categories().categorize("discord"), Category::Social);
    }

    #[test]
    fn legacy_flat_apps_table_still_works() {
        let c = Catalog::from_toml_str("[apps]\nsteam = \"Social\"\n");
        assert_eq!(c.categories().categorize("steam"), Category::Social);
    }

    #[test]
    fn title_policy_defaults_to_off_and_reads_privacy() {
        assert_eq!(catalog().title_policy(), TitlePolicy::Off);
        assert_eq!(
            Catalog::from_toml_str("[privacy]\ntitles = \"off\"\n").title_policy(),
            TitlePolicy::Off
        );
        assert_eq!(
            Catalog::from_toml_str("[privacy]\ntitles = \"hash\"\n").title_policy(),
            TitlePolicy::Hash
        );
        assert_eq!(
            Catalog::from_toml_str("[privacy]\ntitles = \"full\"\n").title_policy(),
            TitlePolicy::Full
        );
        // Unknown values fall back to the safe default.
        assert_eq!(
            Catalog::from_toml_str("[privacy]\ntitles = \"nonsense\"\n").title_policy(),
            TitlePolicy::Off
        );
    }

    #[test]
    fn broken_config_falls_back_to_builtins() {
        let c = Catalog::from_toml_str("this is not = valid = toml [[[");
        assert_eq!(c.categories().categorize("firefox"), Category::Social);
        assert_eq!(c.lookup("t3code").name, "T3code");
    }

    #[test]
    fn lookup_returns_app_info_for_away_and_apps() {
        let c = catalog();
        let info = c.lookup("mpv");
        assert_eq!(info.app_id, "mpv");
        assert_eq!(info.name, "Mpv");
        assert_eq!(info.category_label(), Some("Media"));
        assert!(!info.is_away());
    }

    /// The config we ship must keep parsing: `packaging/install.sh` copies it
    /// straight into `~/.config/actwatch/categories.toml`.
    #[test]
    fn shipped_sample_config_parses() {
        let text = include_str!("../../../packaging/config/categories.toml");
        let c = Catalog::from_toml_str(text);
        assert_eq!(c.title_policy(), TitlePolicy::Off);
        assert_eq!(c.lookup("t3code").name, "T3 Code");
        assert_eq!(c.lookup("org.gnome.Nautilus").name, "Files");
        // Every one of the six categories is reachable from the sample.
        let sample: Vec<(&str, Category)> = vec![
            ("discord", Category::Social),
            ("gnucash", Category::ProductivityFinance),
            ("wofi", Category::Utilities),
            ("steam", Category::Gaming),
            ("mpv", Category::Media),
            ("caelestia-dash", Category::Other),
        ];
        for (app_id, expected) in sample {
            assert_eq!(
                c.categories().categorize(app_id),
                expected,
                "{app_id} should be {expected}"
            );
        }
    }
}