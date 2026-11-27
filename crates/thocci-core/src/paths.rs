//! Runtime paths and persisted settings (Linux, Windows, macOS).
//!
//! State lives in one small directory per user:
//!
//! - Linux:   `$XDG_STATE_HOME/thocci` (default `~/.local/state/thocci`)
//! - Windows: `%LOCALAPPDATA%\thocci`
//! - macOS:   `~/Library/Application Support/thocci`
//!
//! `THOCCI_STATE_DIR` overrides the location (used by tests and portable setups).

use std::fs;
use std::io;
use std::path::PathBuf;

use crate::soundpack::{self, DEFAULT_PACK_ID};

/// Localhost control endpoint. Mechy used 24731; Thocci uses the next port so
/// both can run side by side during the migration.
pub const CONTROL_HOST: &str = "127.0.0.1";
pub const CONTROL_PORT: u16 = 24732;

const DEFAULT_VOLUME: u8 = 100;
const DEFAULT_STEREO: bool = false;

pub fn state_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("THOCCI_STATE_DIR").filter(|dir| !dir.is_empty()) {
        return PathBuf::from(dir);
    }
    platform_state_root().join("thocci")
}

#[cfg(windows)]
fn platform_state_root() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join("AppData").join("Local"))
}

#[cfg(target_os = "macos")]
fn platform_state_root() -> PathBuf {
    home_dir().join("Library").join("Application Support")
}

#[cfg(all(unix, not(target_os = "macos")))]
fn platform_state_root() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".local").join("state"))
}

fn home_dir() -> PathBuf {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(var)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn pid_file() -> PathBuf {
    state_dir().join("thocci.pid")
}

pub fn volume_file() -> PathBuf {
    state_dir().join("volume")
}

pub fn pack_file() -> PathBuf {
    state_dir().join("pack")
}

pub fn stereo_file() -> PathBuf {
    state_dir().join("stereo")
}

pub fn port_file() -> PathBuf {
    state_dir().join("control.port")
}

pub fn log_file() -> PathBuf {
    state_dir().join("thocci.log")
}

pub fn soundpacks_dir() -> PathBuf {
    state_dir().join("soundpacks")
}

pub fn ui_prefs_file() -> PathBuf {
    state_dir().join("ui-prefs.json")
}

pub fn ensure_state_dir() -> io::Result<()> {
    fs::create_dir_all(state_dir())
}

fn read_trimmed(path: PathBuf) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|text| text.trim().to_owned())
}

fn write_value(path: PathBuf, value: &str) -> io::Result<()> {
    ensure_state_dir()?;
    fs::write(path, value)
}

/// Saved volume percent (0-100). Defaults to 100 like Mechy.
pub fn read_saved_volume() -> u8 {
    read_trimmed(volume_file())
        .and_then(|raw| raw.parse::<i64>().ok())
        .map(clamp_percent)
        .unwrap_or(DEFAULT_VOLUME)
}

pub fn write_saved_volume(percent: i64) -> io::Result<()> {
    write_value(volume_file(), &clamp_percent(percent).to_string())
}

pub fn clamp_percent(value: i64) -> u8 {
    value.clamp(0, 100) as u8
}

/// Saved pack id (built-in or user-imported); unknown or missing values fall back to the default.
pub fn read_saved_pack() -> String {
    read_trimmed(pack_file())
        .filter(|id| soundpack::is_known_pack(id))
        .unwrap_or_else(|| DEFAULT_PACK_ID.to_owned())
}

pub fn write_saved_pack(pack_id: &str) -> io::Result<()> {
    if !soundpack::is_known_pack(pack_id) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("Unknown soundpack: {pack_id}"),
        ));
    }
    write_value(pack_file(), pack_id)
}

/// Saved stereo keystroke panning preference. Defaults to `false` (off) for
/// fresh installs, missing files, and unrecognized values.
pub fn read_saved_stereo() -> bool {
    read_trimmed(stereo_file())
        .and_then(|raw| parse_stereo_flag(&raw))
        .unwrap_or(DEFAULT_STEREO)
}

pub fn write_saved_stereo(enabled: bool) -> io::Result<()> {
    write_value(stereo_file(), if enabled { "on" } else { "off" })
}

pub(crate) fn parse_stereo_flag(raw: &str) -> Option<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "on" => Some(true),
        "0" | "false" | "off" => Some(false),
        _ => None,
    }
}

pub fn read_control_port() -> u16 {
    read_trimmed(port_file())
        .and_then(|raw| raw.parse().ok())
        .unwrap_or(CONTROL_PORT)
}

pub fn write_control_port(port: u16) -> io::Result<()> {
    write_value(port_file(), &port.to_string())
}

pub fn read_pid_file() -> Option<u32> {
    read_trimmed(pid_file()).and_then(|raw| raw.parse().ok())
}

/// Remove a file, treating "already gone" as success.
pub fn remove_if_exists(path: PathBuf) {
    if let Err(err) = fs::remove_file(&path) {
        if err.kind() != io::ErrorKind::NotFound {
            log::debug!("could not remove {}: {err}", path.display());
        }
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::path::PathBuf;
    use std::sync::{Mutex, MutexGuard, OnceLock};

    /// Point the whole test process at a private state directory, and
    /// serialize tests that read or write it.
    pub fn isolated_state() -> MutexGuard<'static, PathBuf> {
        static DIR: OnceLock<Mutex<PathBuf>> = OnceLock::new();
        let lock = DIR.get_or_init(|| {
            let dir = std::env::temp_dir().join(format!("thocci-test-{}", std::process::id()));
            std::fs::create_dir_all(&dir).expect("create test state dir");
            std::env::set_var("THOCCI_STATE_DIR", &dir);
            Mutex::new(dir)
        });
        lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_round_trips_and_clamps() {
        let _guard = test_support::isolated_state();
        write_saved_volume(150).unwrap();
        assert_eq!(read_saved_volume(), 100);
        write_saved_volume(-3).unwrap();
        assert_eq!(read_saved_volume(), 0);
        write_saved_volume(42).unwrap();
        assert_eq!(read_saved_volume(), 42);
    }

    #[test]
    fn pack_rejects_unknown_ids() {
        let _guard = test_support::isolated_state();
        assert!(write_saved_pack("nope").is_err());
        write_saved_pack("banana-split").unwrap();
        assert_eq!(read_saved_pack(), "banana-split");
        std::fs::write(pack_file(), "garbage").unwrap();
        assert_eq!(read_saved_pack(), DEFAULT_PACK_ID);
    }

    #[test]
    fn stereo_defaults_off_and_round_trips() {
        let _guard = test_support::isolated_state();
        remove_if_exists(stereo_file());
        assert!(!read_saved_stereo());

        write_saved_stereo(true).unwrap();
        assert!(read_saved_stereo());

        write_saved_stereo(false).unwrap();
        assert!(!read_saved_stereo());

        std::fs::write(stereo_file(), "garbage").unwrap();
        assert!(!read_saved_stereo());
    }
}
