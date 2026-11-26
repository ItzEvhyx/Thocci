//! UI-only preferences persisted separately from daemon engine settings.
//!
//! Engine playback state (`pack`, `volume`, PID, control port) lives in
//! `thocci-core`'s files so the CLI and desktop app stay in lockstep.
//! Desktop UI preferences (`onboarding_completed`, `theme`, `animate_mascot`,
//! and floating dashboard window dimensions) live in `ui-prefs.json` and never
//! overwrite playback settings.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thocci_core::paths;

pub const DEFAULT_DASHBOARD_WIDTH: u32 = 1160;
pub const DEFAULT_DASHBOARD_HEIGHT: u32 = 806;
pub const MIN_DASHBOARD_WIDTH: u32 = 640;
pub const MIN_DASHBOARD_HEIGHT: u32 = 520;
pub const MAX_DASHBOARD_WIDTH: u32 = 3840;
pub const MAX_DASHBOARD_HEIGHT: u32 = 2160;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiPrefs {
    pub onboarding_completed: bool,
    /// `None` means follow the OS appearance until the user explicitly chooses
    /// `"light"` or `"dark"`.
    pub theme: Option<String>,
    pub animate_mascot: bool,
    /// Optional preferred dashboard width in logical pixels (defaults to 1160).
    pub window_width: Option<u32>,
    /// Optional preferred dashboard height in logical pixels (defaults to 806).
    pub window_height: Option<u32>,
}

impl Default for UiPrefs {
    fn default() -> Self {
        Self {
            onboarding_completed: false,
            theme: None,
            animate_mascot: true,
            window_width: None,
            window_height: None,
        }
    }
}

impl UiPrefs {
    pub fn sanitized(mut self) -> Self {
        self.theme = match self.theme.as_deref() {
            Some("light") => Some("light".to_owned()),
            Some("dark") => Some("dark".to_owned()),
            _ => None,
        };
        self.window_width = self
            .window_width
            .map(|w| w.clamp(MIN_DASHBOARD_WIDTH, MAX_DASHBOARD_WIDTH));
        self.window_height = self
            .window_height
            .map(|h| h.clamp(MIN_DASHBOARD_HEIGHT, MAX_DASHBOARD_HEIGHT));
        self
    }

    /// Resolve effective dashboard size (checking `THOCCI_WINDOW_SIZE=WxH`,
    /// then saved preferences, then the approved 1160x806 default).
    pub fn effective_dashboard_size(&self) -> (u32, u32) {
        if let Some((w, h)) = parse_env_window_size() {
            return (
                w.clamp(MIN_DASHBOARD_WIDTH, MAX_DASHBOARD_WIDTH),
                h.clamp(MIN_DASHBOARD_HEIGHT, MAX_DASHBOARD_HEIGHT),
            );
        }
        (
            self.window_width
                .unwrap_or(DEFAULT_DASHBOARD_WIDTH)
                .clamp(MIN_DASHBOARD_WIDTH, MAX_DASHBOARD_WIDTH),
            self.window_height
                .unwrap_or(DEFAULT_DASHBOARD_HEIGHT)
                .clamp(MIN_DASHBOARD_HEIGHT, MAX_DASHBOARD_HEIGHT),
        )
    }
}

fn parse_env_window_size() -> Option<(u32, u32)> {
    let raw = std::env::var("THOCCI_WINDOW_SIZE").ok()?;
    let trimmed = raw.trim();
    let (w_str, h_str) = trimmed
        .split_once('x')
        .or_else(|| trimmed.split_once('X'))?;
    let w = w_str.trim().parse::<u32>().ok()?;
    let h = h_str.trim().parse::<u32>().ok()?;
    Some((w, h))
}

pub fn load_prefs() -> UiPrefs {
    let path = paths::ui_prefs_file();
    let Ok(bytes) = fs::read(&path) else {
        return UiPrefs::default();
    };
    serde_json::from_slice::<UiPrefs>(&bytes)
        .map(UiPrefs::sanitized)
        .unwrap_or_default()
}

pub fn save_prefs(prefs: &UiPrefs) -> Result<UiPrefs, String> {
    let clean = prefs.clone().sanitized();
    paths::ensure_state_dir().map_err(|err| err.to_string())?;

    let target = paths::ui_prefs_file();
    let tmp = temp_path_for(&target);
    let data = serde_json::to_vec_pretty(&clean).map_err(|err| err.to_string())?;
    fs::write(&tmp, data).map_err(|err| err.to_string())?;
    if let Err(err) = fs::rename(&tmp, &target) {
        let _ = fs::remove_file(&tmp);
        return Err(err.to_string());
    }
    Ok(clean)
}

fn temp_path_for(target: &std::path::Path) -> PathBuf {
    let file_name = target
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("ui-prefs.json");
    target.with_file_name(format!(".{file_name}.{}.tmp", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_prefs_round_trip_without_touching_engine_settings() {
        let dir = std::env::temp_dir().join(format!("thocci-ui-prefs-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        std::env::set_var("THOCCI_STATE_DIR", &dir);

        paths::write_saved_pack("creamy").unwrap();
        paths::write_saved_volume(42).unwrap();

        let defaults = load_prefs();
        assert!(!defaults.onboarding_completed);
        assert_eq!(defaults.theme, None);
        assert!(defaults.animate_mascot);
        assert_eq!(
            defaults.effective_dashboard_size(),
            (DEFAULT_DASHBOARD_WIDTH, DEFAULT_DASHBOARD_HEIGHT)
        );

        let saved = save_prefs(&UiPrefs {
            onboarding_completed: true,
            theme: Some("dark".to_owned()),
            animate_mascot: false,
            window_width: Some(1240),
            window_height: Some(840),
        })
        .unwrap();
        assert_eq!(load_prefs(), saved);
        assert_eq!(saved.effective_dashboard_size(), (1240, 840));

        // Engine settings remain untouched.
        assert_eq!(paths::read_saved_pack(), "creamy");
        assert_eq!(paths::read_saved_volume(), 42);

        let _ = fs::remove_dir_all(&dir);
    }
}
