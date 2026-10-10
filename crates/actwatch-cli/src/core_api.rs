//! The single seam between the CLI and `actwatch-core`.
//!
//! Display names, categories, the title policy and the reserved `"away"` id
//! all come from `actwatch-core`, so every frontend agrees and the CLI owns
//! no maps of its own. Only [`Catalog`] touches the core types; renderers
//! take an `&Catalog` and never re-derive a name or a category.

pub use actwatch_core::{Categories, Category, DisplayNames, TitlePolicy};

/// Reserved app id for idle / lock spans (v2 contract, "Away + permissions").
/// Frontends keep it out of app totals and render it as its own row.
pub const AWAY_APP_ID: &str = "away";

/// Label for [`AWAY_APP_ID`] in every renderer.
pub const AWAY_LABEL: &str = "Away";

/// Everything the CLI needs to turn a raw `app_id` into something a person
/// wants to read. Cheap to build once per process and immutable afterwards.
#[derive(Debug, Clone)]
pub struct Catalog {
    names: DisplayNames,
    categories: Categories,
    policy: TitlePolicy,
}

impl Catalog {
    /// Builds the catalog for this machine. Never fails: a missing, unreadable
    /// or malformed config falls back to the builtins, because a broken config
    /// must not stop someone from reading their own history.
    pub fn load() -> Self {
        Self {
            names: DisplayNames::load(),
            categories: Categories::load(),
            policy: TitlePolicy::load(),
        }
    }

    /// Builds a catalog from TOML text without touching the filesystem
    /// (no `.desktop` scan). Test seam: production goes through [`Catalog::load`].
    #[cfg(test)]
    pub fn from_toml_str(text: &str) -> Self {
        let mut names = DisplayNames::default();
        let _ = names.apply_toml_str(text);
        Self {
            names,
            categories: Categories::from_toml_str(text)
                .unwrap_or_else(|_| Categories::builtin()),
            policy: TitlePolicy::from_toml_str(text).unwrap_or_default(),
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

    /// True for idle/lock spans. Both the v1 (`idle`, lock-class names) and
    /// v2 (`away`) ids count, so old history groups with new history.
    pub fn is_away(app_id: &str) -> bool {
        matches!(
            app_id.to_lowercase().as_str(),
            "away" | "idle" | "lockscreen" | "hyprlock" | "swaylock" | "waylock"
        )
    }

    /// Configured window-title policy. The CLI is app-level only and never
    /// renders titles; this exists so `status` can tell the truth about what
    /// the daemon recorded.
    pub fn title_policy(&self) -> TitlePolicy {
        self.policy
    }

    #[allow(dead_code)] // inspection accessor for tests
    pub fn categories(&self) -> &Categories {
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
        let dir = std::env::temp_dir().join(format!(
            "actwatch-cli-test-{}/desktop",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("t3code.desktop"),
            "[Desktop Entry]\nName=T3 Code\nStartupWMClass=T3Code\nExec=t3code\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("customapp.desktop"),
            "[Desktop Entry]\nName=Custom App\n",
        )
        .unwrap();
        let names = DisplayNames::load_from_dirs(
            &[dir.as_path()],
            std::collections::HashMap::new(),
        );
        assert_eq!(names.resolve("T3Code"), "T3 Code");
        // Bare stem also matches a reverse-DNS id without a WMClass.
        assert_eq!(names.resolve("com.vendor.customapp"), "Custom App");
        let _ = std::fs::remove_dir_all(&dir);
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
        let dir = std::env::temp_dir().join(format!(
            "actwatch-cli-test-{}/aliases",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("t3code.desktop"),
            "[Desktop Entry]\nName=T3 Code\n",
        )
        .unwrap();
        let mut aliases = std::collections::HashMap::new();
        aliases.insert("t3code".to_string(), "The Code".to_string());
        let names = DisplayNames::load_from_dirs(&[dir.as_path()], aliases);
        assert_eq!(names.resolve("t3code"), "The Code");
        let _ = std::fs::remove_dir_all(&dir);
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
        assert_eq!(Category::all().len(), 6);
        let labels: Vec<&str> = Category::all().iter().map(|c| c.as_str()).collect();
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