pub use crate::core_shim::model::{AppEvent, ResourceSample};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveWindowInfo {
    pub app_id: String,
    pub title: String,
    pub pid: Option<u32>,
}

impl ActiveWindowInfo {
    pub fn away() -> Self {
        Self {
            app_id: "away".to_string(),
            title: String::new(),
            pid: None,
        }
    }

    pub fn is_away(&self) -> bool {
        self.app_id == "away" || self.app_id.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_active_window_away() {
        let win = ActiveWindowInfo::away();
        assert_eq!(win.app_id, "away");
        assert!(win.is_away());
    }

    #[test]
    fn test_active_window_app() {
        let win = ActiveWindowInfo {
            app_id: "firefox".to_string(),
            title: "Mozilla Firefox".to_string(),
            pid: Some(1234),
        };
        assert!(!win.is_away());
    }
}
