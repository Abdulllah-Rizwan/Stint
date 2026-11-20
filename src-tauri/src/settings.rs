use serde::{Deserialize, Serialize};

/// User-configurable settings. Persisted as JSON in the `settings` table.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Seconds without keyboard/mouse input before the user counts as away.
    pub idle_threshold_secs: u32,
    /// Store window titles (e.g. document or tab names). Off by default for privacy.
    pub capture_titles: bool,
    /// App names that are never recorded (case-insensitive exact match).
    pub excluded_apps: Vec<String>,
    /// Minimum length of an automatically detected focus session.
    pub focus_min_minutes: u32,
    /// Interruptions shorter than this do not end a focus session.
    pub focus_break_tolerance_minutes: u32,
    /// Delete activity older than this many days. 0 keeps everything.
    pub retention_days: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            idle_threshold_secs: 300,
            capture_titles: false,
            // Lock screens and screen savers are not work.
            excluded_apps: vec![
                "loginwindow".into(),
                "ScreenSaverEngine".into(),
                "LockApp".into(),
            ],
            focus_min_minutes: 25,
            focus_break_tolerance_minutes: 5,
            retention_days: 0,
        }
    }
}

pub const MAX_APP_NAME_LEN: usize = 128;
const MAX_EXCLUDED: usize = 200;

impl Settings {
    /// Reject out-of-range values coming from the UI.
    pub fn validate(&self) -> Result<(), String> {
        if !(30..=3600).contains(&self.idle_threshold_secs) {
            return Err("Idle threshold must be between 30 seconds and 60 minutes".into());
        }
        if !(5..=240).contains(&self.focus_min_minutes) {
            return Err("Minimum focus length must be between 5 and 240 minutes".into());
        }
        if !(1..=30).contains(&self.focus_break_tolerance_minutes) {
            return Err("Break tolerance must be between 1 and 30 minutes".into());
        }
        if self.retention_days > 3650 {
            return Err("Retention must be at most 3650 days".into());
        }
        if self.excluded_apps.len() > MAX_EXCLUDED {
            return Err(format!("At most {MAX_EXCLUDED} excluded apps are allowed"));
        }
        Ok(())
    }

    /// Trim, de-duplicate and cap the excluded app list.
    pub fn normalized(mut self) -> Self {
        let mut seen = std::collections::HashSet::new();
        self.excluded_apps = self
            .excluded_apps
            .into_iter()
            .map(|s| clean_text(&s, MAX_APP_NAME_LEN))
            .filter(|s| !s.is_empty() && seen.insert(s.to_lowercase()))
            .take(MAX_EXCLUDED)
            .collect();
        self
    }

    pub fn is_excluded(&self, app: &str) -> bool {
        self.excluded_apps
            .iter()
            .any(|e| e.eq_ignore_ascii_case(app))
    }
}

/// Remove control characters, trim, and cap length (by characters, not bytes).
pub fn clean_text(s: &str, max_chars: usize) -> String {
    s.chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .chars()
        .take(max_chars)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        assert!(Settings::default().validate().is_ok());
    }

    #[test]
    fn rejects_out_of_range() {
        let s = Settings {
            idle_threshold_secs: 1,
            ..Default::default()
        };
        assert!(s.validate().is_err());
    }

    #[test]
    fn normalizes_exclusions() {
        let s = Settings {
            excluded_apps: vec![
                " Slack ".into(),
                "slack".into(),
                "".into(),
                "a\u{0007}b".into(),
            ],
            ..Default::default()
        }
        .normalized();
        assert_eq!(s.excluded_apps, vec!["Slack".to_string(), "ab".to_string()]);
        assert!(s.is_excluded("SLACK"));
    }
}
