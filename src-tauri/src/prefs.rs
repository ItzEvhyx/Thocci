//! UI-only preferences persisted separately from daemon engine settings.
//!
//! Engine playback state (`pack`, `volume`, PID, control port) lives in
//! `thocci-core`'s files so the CLI and desktop app stay in lockstep.
//! Desktop UI preferences (`onboarding_completed`, `theme`, `animate_mascot`,
//! floating dashboard window dimensions, and `pinned_packs`) live in
//! `ui-prefs.json` and never overwrite playback settings.
//!
//! All reads-modify-writes serialize through a single process-wide lock and
//! support field-specific patches so simultaneous pin changes, theme saves, and
//! window resizes cannot overwrite each other.

use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Deserializer, Serialize};
use thocci_core::paths;

pub const DEFAULT_DASHBOARD_WIDTH: u32 = 1160;
pub const DEFAULT_DASHBOARD_HEIGHT: u32 = 806;
pub const MIN_DASHBOARD_WIDTH: u32 = 480;
pub const MIN_DASHBOARD_HEIGHT: u32 = 440;
pub const MAX_DASHBOARD_WIDTH: u32 = 3840;
pub const MAX_DASHBOARD_HEIGHT: u32 = 2160;
pub const MAX_PINNED_PACKS: usize = 10;

static PREFS_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

fn prefs_lock() -> &'static Mutex<()> {
    PREFS_LOCK.get_or_init(|| Mutex::new(()))
}

/// Fit a desired dashboard window size cleanly inside the monitor's logical
/// work area (leaving a comfortable margin around taskbars/panels). Scales
/// proportionally when the monitor is smaller than the default 1160x806 so
/// the entire window stays visible on 1366x768 and 150%-DPI laptops.
pub fn fit_dashboard_size_to_work_area(
    desired_w: u32,
    desired_h: u32,
    work_w: u32,
    work_h: u32,
) -> (u32, u32) {
    if work_w == 0 || work_h == 0 {
        return (desired_w, desired_h);
    }
    let margin_x = if work_w > 560 { 32 } else { 12 };
    let margin_y = if work_h > 500 { 32 } else { 12 };
    let max_w = work_w.saturating_sub(margin_x).max(320);
    let max_h = work_h.saturating_sub(margin_y).max(320);

    if desired_w <= max_w && desired_h <= max_h {
        return (desired_w, desired_h);
    }

    let scale_w = f64::from(max_w) / f64::from(desired_w.max(1));
    let scale_h = f64::from(max_h) / f64::from(desired_h.max(1));
    let scale = scale_w.min(scale_h).clamp(0.1, 1.0);

    let min_w = MIN_DASHBOARD_WIDTH.min(max_w);
    let min_h = MIN_DASHBOARD_HEIGHT.min(max_h);
    let fitted_w = ((f64::from(desired_w) * scale).round() as u32).clamp(min_w, max_w);
    let fitted_h = ((f64::from(desired_h) * scale).round() as u32).clamp(min_h, max_h);
    (fitted_w, fitted_h)
}

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
    /// Ordered, deduplicated list of up to 10 pinned soundpack IDs.
    pub pinned_packs: Vec<String>,
}

impl Default for UiPrefs {
    fn default() -> Self {
        Self {
            onboarding_completed: false,
            theme: None,
            animate_mascot: true,
            window_width: None,
            window_height: None,
            pinned_packs: Vec::new(),
        }
    }
}

/// Partial update applied atomically against the latest saved [`UiPrefs`].
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct UiPrefsPatch {
    pub onboarding_completed: Option<bool>,
    #[serde(deserialize_with = "deserialize_double_option")]
    pub theme: Option<Option<String>>,
    pub animate_mascot: Option<bool>,
    #[serde(deserialize_with = "deserialize_double_option")]
    pub window_width: Option<Option<u32>>,
    #[serde(deserialize_with = "deserialize_double_option")]
    pub window_height: Option<Option<u32>>,
    pub pinned_packs: Option<Vec<String>>,
}

fn deserialize_double_option<'de, T, D>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    Deserialize::deserialize(deserializer).map(Some)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PinMutationStatus {
    Pinned,
    Unpinned,
    Limit,
    Unchanged,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PinMutationResult {
    pub prefs: UiPrefs,
    pub status: PinMutationStatus,
}

pub fn sanitize_pinned_packs(raw: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::with_capacity(MAX_PINNED_PACKS);
    let mut out = Vec::with_capacity(MAX_PINNED_PACKS);
    for entry in raw {
        let trimmed = entry.trim();
        if trimmed.is_empty() {
            continue;
        }
        if seen.insert(trimmed.to_owned()) {
            out.push(trimmed.to_owned());
            if out.len() >= MAX_PINNED_PACKS {
                break;
            }
        }
    }
    out
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
        self.pinned_packs = sanitize_pinned_packs(self.pinned_packs);
        self
    }

    pub fn apply_patch(&mut self, patch: UiPrefsPatch) {
        if let Some(onboarding_completed) = patch.onboarding_completed {
            self.onboarding_completed = onboarding_completed;
        }
        if let Some(theme) = patch.theme {
            self.theme = theme;
        }
        if let Some(animate_mascot) = patch.animate_mascot {
            self.animate_mascot = animate_mascot;
        }
        if let Some(window_width) = patch.window_width {
            self.window_width = window_width;
        }
        if let Some(window_height) = patch.window_height {
            self.window_height = window_height;
        }
        if let Some(pinned_packs) = patch.pinned_packs {
            self.pinned_packs = pinned_packs;
        }
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

fn load_prefs_unlocked() -> UiPrefs {
    let path = paths::ui_prefs_file();
    let Ok(bytes) = fs::read(&path) else {
        return UiPrefs::default();
    };
    serde_json::from_slice::<UiPrefs>(&bytes)
        .map(UiPrefs::sanitized)
        .unwrap_or_default()
}

fn save_prefs_unlocked(prefs: &UiPrefs) -> Result<UiPrefs, String> {
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

pub fn load_prefs() -> UiPrefs {
    let _guard = prefs_lock().lock().unwrap_or_else(|e| e.into_inner());
    load_prefs_unlocked()
}

#[cfg(test)]
pub fn save_prefs(prefs: &UiPrefs) -> Result<UiPrefs, String> {
    let _guard = prefs_lock().lock().unwrap_or_else(|e| e.into_inner());
    save_prefs_unlocked(prefs)
}

/// Atomically apply a partial [`UiPrefsPatch`] against the latest saved state.
pub fn patch_prefs(patch: UiPrefsPatch) -> Result<UiPrefs, String> {
    update_prefs(|current| {
        current.apply_patch(patch);
        Ok(())
    })
    .map(|(saved, _)| saved)
}

/// Atomically read, mutate, sanitize, and persist [`UiPrefs`] under the shared
/// preferences lock.
pub fn update_prefs<F, R>(mutate: F) -> Result<(UiPrefs, R), String>
where
    F: FnOnce(&mut UiPrefs) -> Result<R, String>,
{
    let _guard = prefs_lock().lock().unwrap_or_else(|e| e.into_inner());
    let mut current = load_prefs_unlocked();
    let outcome = mutate(&mut current)?;
    let saved = save_prefs_unlocked(&current)?;
    Ok((saved, outcome))
}

/// Pin or unpin a single soundpack atomically against the latest saved prefs,
/// enforcing deduplication, pin order preservation, and the 10-pack cap.
pub fn set_pack_pinned<F>(
    pack_id: &str,
    pinned: bool,
    is_known_pack: F,
) -> Result<PinMutationResult, String>
where
    F: Fn(&str) -> bool,
{
    let id = pack_id.trim();
    if id.is_empty() {
        return Err("Soundpack ID cannot be empty.".to_owned());
    }

    let _guard = prefs_lock().lock().unwrap_or_else(|e| e.into_inner());
    let mut current = load_prefs_unlocked();

    if !pinned {
        let before = current.pinned_packs.len();
        current.pinned_packs.retain(|existing| existing != id);
        if current.pinned_packs.len() == before {
            return Ok(PinMutationResult {
                prefs: current,
                status: PinMutationStatus::Unchanged,
            });
        }
        let saved = save_prefs_unlocked(&current)?;
        return Ok(PinMutationResult {
            prefs: saved,
            status: PinMutationStatus::Unpinned,
        });
    }

    if !is_known_pack(id) {
        return Err(format!("Unknown soundpack '{id}'."));
    }

    if current.pinned_packs.iter().any(|existing| existing == id) {
        return Ok(PinMutationResult {
            prefs: current,
            status: PinMutationStatus::Unchanged,
        });
    }

    // Drop stale IDs (e.g. removed imported packs) before checking the 10-pin cap
    // so deleted packs never permanently occupy slots.
    let active_pins: Vec<String> = current
        .pinned_packs
        .iter()
        .filter(|existing| is_known_pack(existing))
        .cloned()
        .collect();

    if active_pins.len() >= MAX_PINNED_PACKS {
        return Ok(PinMutationResult {
            prefs: current,
            status: PinMutationStatus::Limit,
        });
    }

    current.pinned_packs = active_pins;
    current.pinned_packs.push(id.to_owned());
    let saved = save_prefs_unlocked(&current)?;
    Ok(PinMutationResult {
        prefs: saved,
        status: PinMutationStatus::Pinned,
    })
}

fn temp_path_for(target: &std::path::Path) -> PathBuf {
    let file_name = target
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("ui-prefs.json");
    let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
    target.with_file_name(format!(".{file_name}.{}.{seq}.tmp", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn isolated_prefs_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "thocci-ui-prefs-{label}-{}-{}",
            std::process::id(),
            TEMP_SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        std::env::set_var("THOCCI_STATE_DIR", &dir);
        dir
    }

    fn test_env_lock() -> &'static Mutex<()> {
        static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        ENV_LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn ui_prefs_round_trip_without_touching_engine_settings() {
        let _env = test_env_lock().lock().unwrap_or_else(|e| e.into_inner());
        let dir = isolated_prefs_dir("roundtrip");

        paths::write_saved_pack("creamy").unwrap();
        paths::write_saved_volume(42).unwrap();

        let defaults = load_prefs();
        assert!(!defaults.onboarding_completed);
        assert_eq!(defaults.theme, None);
        assert!(defaults.animate_mascot);
        assert!(defaults.pinned_packs.is_empty());
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
            pinned_packs: vec!["creamy".to_owned(), "banana-split".to_owned()],
        })
        .unwrap();
        assert_eq!(load_prefs(), saved);
        assert_eq!(saved.effective_dashboard_size(), (1240, 840));
        assert_eq!(saved.pinned_packs, vec!["creamy", "banana-split"]);

        // Engine settings remain untouched.
        assert_eq!(paths::read_saved_pack(), "creamy");
        assert_eq!(paths::read_saved_volume(), 42);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn legacy_prefs_without_pinned_packs_loads_empty_and_preserves_other_fields() {
        let _env = test_env_lock().lock().unwrap_or_else(|e| e.into_inner());
        let dir = isolated_prefs_dir("legacy");
        let legacy_json = r#"{
            "onboarding_completed": true,
            "theme": "dark",
            "animate_mascot": false,
            "window_width": 1200,
            "window_height": 780
        }"#;
        fs::write(paths::ui_prefs_file(), legacy_json).unwrap();

        let loaded = load_prefs();
        assert!(loaded.onboarding_completed);
        assert_eq!(loaded.theme.as_deref(), Some("dark"));
        assert!(!loaded.animate_mascot);
        assert_eq!(loaded.window_width, Some(1200));
        assert_eq!(loaded.window_height, Some(780));
        assert!(loaded.pinned_packs.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn pins_preserve_order_deduplicate_cap_at_ten_and_ignore_stale_on_capacity() {
        let _env = test_env_lock().lock().unwrap_or_else(|e| e.into_inner());
        let dir = isolated_prefs_dir("pins");

        let known = |id: &str| id.starts_with("pack-") || id == "imported-one";

        // Seed a stale ID directly on disk alongside a valid pin.
        save_prefs(&UiPrefs {
            onboarding_completed: true,
            theme: Some("light".to_owned()),
            animate_mascot: true,
            window_width: Some(1100),
            window_height: Some(760),
            pinned_packs: vec!["stale-deleted-pack".to_owned(), "pack-1".to_owned()],
        })
        .unwrap();

        // Pin 9 more unique packs (including an imported pack) -> total 10 valid packs
        for idx in 2..=9 {
            let res = set_pack_pinned(&format!("pack-{idx}"), true, known).unwrap();
            assert_eq!(res.status, PinMutationStatus::Pinned);
        }
        let res_imp = set_pack_pinned("imported-one", true, known).unwrap();
        assert_eq!(res_imp.status, PinMutationStatus::Pinned);
        assert_eq!(res_imp.prefs.pinned_packs.len(), 10);
        assert_eq!(res_imp.prefs.pinned_packs[0], "pack-1");
        assert_eq!(res_imp.prefs.pinned_packs[9], "imported-one");

        // Duplicate pin is a no-op
        let dup = set_pack_pinned("pack-3", true, known).unwrap();
        assert_eq!(dup.status, PinMutationStatus::Unchanged);
        assert_eq!(dup.prefs.pinned_packs.len(), 10);

        // 11th unique pack is rejected cleanly without altering list or other prefs
        let over = set_pack_pinned("pack-11", true, known).unwrap();
        assert_eq!(over.status, PinMutationStatus::Limit);
        assert_eq!(over.prefs.pinned_packs.len(), 10);
        assert_eq!(over.prefs.theme.as_deref(), Some("light"));

        // Field-specific theme patch does not clobber pinned_packs or window size
        let patched = patch_prefs(UiPrefsPatch {
            theme: Some(Some("dark".to_owned())),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(patched.theme.as_deref(), Some("dark"));
        assert_eq!(patched.window_width, Some(1100));
        assert_eq!(patched.pinned_packs.len(), 10);

        // Unpin preserves order of remaining items and frees a slot
        let unpinned = set_pack_pinned("pack-4", false, known).unwrap();
        assert_eq!(unpinned.status, PinMutationStatus::Unpinned);
        assert_eq!(unpinned.prefs.pinned_packs.len(), 9);
        assert_eq!(
            unpinned.prefs.pinned_packs,
            vec![
                "pack-1",
                "pack-2",
                "pack-3",
                "pack-5",
                "pack-6",
                "pack-7",
                "pack-8",
                "pack-9",
                "imported-one"
            ]
        );

        let repinned = set_pack_pinned("pack-11", true, known).unwrap();
        assert_eq!(repinned.status, PinMutationStatus::Pinned);
        assert_eq!(repinned.prefs.pinned_packs.last().unwrap(), "pack-11");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn fits_dashboard_cleanly_inside_low_resolution_work_areas() {
        // Full HD desktop (1920x1040 work area) keeps exact default 1160x806
        assert_eq!(
            fit_dashboard_size_to_work_area(1160, 806, 1920, 1040),
            (1160, 806)
        );

        // 1366x768 laptop with taskbar (~1366x720 work area) scales down cleanly
        let (w768, h768) = fit_dashboard_size_to_work_area(1160, 806, 1366, 720);
        assert!(w768 <= 1334 && h768 <= 688, "got {w768}x{h768}");
        assert!(w768 >= 900 && h768 >= 640, "got {w768}x{h768}");

        // 1920x1080 @ 150% DPI (~1280x672 logical work area) fits with zero cutoff
        let (w_dpi, h_dpi) = fit_dashboard_size_to_work_area(1160, 806, 1280, 672);
        assert!(w_dpi <= 1248 && h_dpi <= 640, "got {w_dpi}x{h_dpi}");
        assert!(w_dpi >= 850 && h_dpi >= 600, "got {w_dpi}x{h_dpi}");
    }
}
