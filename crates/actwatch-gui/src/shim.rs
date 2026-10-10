

// SHIM for core API missing in actwatch-core v1 (feat/gui-bw branch)
// Flagging that we need to replace this once core is fully updated to v2.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Category {
    Social,
    ProductivityFinance,
    Utilities,
    Gaming,
    Media,
    Other,
}

impl Category {
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


}

pub struct Categories;

impl Categories {
    pub fn new() -> Self {
        Self
    }

    pub fn categorize(&self, app_id: &str) -> Category {
        let app_id = app_id.to_lowercase();
        if app_id.contains("discord") || app_id.contains("telegram") || app_id.contains("slack") || app_id.contains("firefox") || app_id.contains("chrome") || app_id.contains("brave") || app_id.contains("zen") {
            Category::Social
        } else if app_id.contains("code") || app_id.contains("nvim") || app_id.contains("term") || app_id.contains("obsidian") || app_id.contains("office") {
            Category::ProductivityFinance
        } else if app_id.contains("top") || app_id.contains("sys") || app_id.contains("monitor") || app_id.contains("settings") || app_id.contains("file") || app_id.contains("dolphin") || app_id.contains("nautilus") {
            Category::Utilities
        } else if app_id.contains("steam") || app_id.contains("lutris") || app_id.contains("heroic") || app_id.contains("bottles") {
            Category::Gaming
        } else if app_id.contains("vlc") || app_id.contains("mpv") || app_id.contains("spotify") || app_id.contains("obs") {
            Category::Media
        } else {
            Category::Other
        }
    }
}

pub struct DisplayNames;

impl DisplayNames {
    pub fn load() -> Self {
        Self
    }

    pub fn resolve(&self, app_id: &str) -> String {
        if app_id == "away" {
            return "Away".to_string();
        }
        
        let mut s = app_id.to_string();
        if let Some(idx) = s.rfind('.') {
            s = s[idx + 1..].to_string();
        }
        s = s.replace(['-', '_', '.'], " ");
        let mut out = String::new();
        let mut capitalize = true;
        for c in s.chars() {
            if c.is_whitespace() {
                capitalize = true;
                out.push(c);
            } else if capitalize {
                out.extend(c.to_uppercase());
                capitalize = false;
            } else {
                out.push(c);
            }
        }
        if out.is_empty() {
            "Unknown".to_string()
        } else {
            out
        }
    }
}
