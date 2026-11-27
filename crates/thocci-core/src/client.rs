//! High-level daemon client shared by the `thocci` CLI and the Tauri app.

use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::control::send_command;
use crate::paths::{
    clamp_percent, ensure_state_dir, log_file, parse_stereo_flag, pid_file, port_file,
    read_pid_file, read_saved_pack, read_saved_stereo, read_saved_volume, remove_if_exists,
    write_saved_pack, write_saved_stereo, write_saved_volume,
};
use crate::process::{pid_is_running, spawn_detached, terminate_pid};
use crate::soundpack::{
    self, is_known_pack, known_pack_ids, BuiltinPackMeta, PackCatalogEntry, Soundpack,
    BUILTIN_PACKS,
};
use crate::{Error, Result};

/// Snapshot of daemon status for both CLI display and Tauri UI state.
#[derive(Debug, Clone, Serialize)]
pub struct DaemonStatus {
    pub running: bool,
    pub responsive: bool,
    pub pid: Option<u32>,
    pub volume: u8,
    pub pack: String,
    pub stereo: bool,
    pub saved_volume: u8,
    pub saved_pack: String,
    pub saved_stereo: bool,
}

#[derive(Debug, Clone, Default)]
pub struct StartOptions {
    pub pack_dir: Option<PathBuf>,
    pub volume: Option<i64>,
}

/// Return the PID of a live daemon, cleaning up a stale PID file if needed.
pub fn running_pid() -> Option<u32> {
    let pid = read_pid_file()?;
    if pid_is_running(pid) {
        Some(pid)
    } else {
        remove_if_exists(pid_file());
        None
    }
}

/// Query the live daemon (or fall back to saved settings if stopped).
pub fn status() -> DaemonStatus {
    let saved_pack = read_saved_pack();
    let saved_volume = read_saved_volume();
    let saved_stereo = read_saved_stereo();
    let Some(pid) = running_pid() else {
        return DaemonStatus {
            running: false,
            responsive: false,
            pid: None,
            volume: saved_volume,
            pack: saved_pack.clone(),
            stereo: saved_stereo,
            saved_volume,
            saved_pack,
            saved_stereo,
        };
    };

    if let Some(reply) = send_command("status", Duration::from_secs(1)) {
        if let Some(parsed) =
            parse_status_reply(&reply, pid, saved_volume, &saved_pack, saved_stereo)
        {
            return parsed;
        }
    }

    // Do not report panning as confirmed active while a running daemon is unresponsive.
    DaemonStatus {
        running: true,
        responsive: false,
        pid: Some(pid),
        volume: saved_volume,
        pack: saved_pack.clone(),
        stereo: false,
        saved_volume,
        saved_pack,
        saved_stereo,
    }
}

/// Return `true` if a live daemon exists and answers `ping` over the control port.
pub fn is_responsive() -> bool {
    running_pid().is_some()
        && send_command("ping", Duration::from_millis(500))
            .as_deref()
            .is_some_and(|r| r.starts_with("OK"))
}

/// Start the background daemon using `daemon_exe` (normally the `thocci` CLI binary).
pub fn start(daemon_exe: &Path, options: StartOptions) -> Result<DaemonStatus> {
    if running_pid().is_some() {
        if is_responsive() {
            return Ok(status());
        }
        // Clean up an unresponsive/wedged daemon process before starting a fresh one.
        let _ = stop();
    }

    ensure_state_dir()?;
    // Clear any stale control/PID files left behind by a previous crashed or killed daemon.
    remove_if_exists(pid_file());
    remove_if_exists(port_file());

    if let Some(dir) = &options.pack_dir {
        if !dir.join("config.json").is_file() {
            return Err(Error::new(format!(
                "soundpack not found at {}",
                dir.display()
            )));
        }
    }

    let volume = match options.volume {
        Some(level) => {
            let clamped = clamp_percent(level);
            write_saved_volume(i64::from(clamped))?;
            clamped
        }
        None => {
            let saved = read_saved_volume();
            write_saved_volume(i64::from(saved))?;
            saved
        }
    };

    let pack_id = read_saved_pack();
    let mut args = vec![
        "daemon".to_owned(),
        "--volume".to_owned(),
        volume.to_string(),
    ];
    if let Some(dir) = &options.pack_dir {
        args.push("--pack".to_owned());
        args.push(dir.to_string_lossy().into_owned());
    } else {
        args.push("--pack-id".to_owned());
        args.push(pack_id.clone());
    }

    let log_path = log_file();
    let mut child = spawn_detached(daemon_exe, &args, &log_path)?;

    let deadline = Instant::now() + Duration::from_secs(8);
    while Instant::now() < deadline {
        thread::sleep(Duration::from_millis(50));
        if let Ok(Some(exit_status)) = child.try_wait() {
            let tail = read_log_tail(&log_path, 8);
            let detail = if tail.is_empty() {
                String::new()
            } else {
                format!("\n{tail}")
            };
            return Err(Error::new(format!(
                "daemon exited early ({exit_status}). See {}{detail}",
                log_path.display()
            )));
        }
        if port_file().is_file() {
            if let Some(reply) = send_command("ping", Duration::from_millis(250)) {
                if reply.starts_with("OK") {
                    // Reap the spawned daemon child asynchronously when it later exits
                    // so long-lived hosts (like `thocci-app`) never accumulate zombies.
                    let _ = thread::Builder::new()
                        .name("thocci-reaper".to_owned())
                        .spawn(move || {
                            let _ = child.wait();
                        });
                    return Ok(status());
                }
            }
        }
    }

    let _ = child.kill();
    let _ = child.wait();
    remove_if_exists(pid_file());
    remove_if_exists(port_file());

    let tail = read_log_tail(&log_path, 8);
    let detail = if tail.is_empty() {
        String::new()
    } else {
        format!("\n{tail}")
    };
    Err(Error::new(format!(
        "daemon did not become ready. See {}{detail}",
        log_path.display()
    )))
}

/// Stop the running daemon gracefully, escalating to a force kill if needed.
/// Returns `true` if a running daemon was stopped, `false` if already stopped.
pub fn stop() -> Result<bool> {
    let Some(pid) = running_pid() else {
        remove_if_exists(pid_file());
        remove_if_exists(port_file());
        return Ok(false);
    };

    if send_command("stop", Duration::from_secs(1)).is_none() {
        terminate_pid(pid, false)?;
    }

    for _ in 0..50 {
        thread::sleep(Duration::from_millis(50));
        if running_pid().is_none() {
            remove_if_exists(pid_file());
            remove_if_exists(port_file());
            return Ok(true);
        }
    }

    let _ = terminate_pid(pid, true);
    remove_if_exists(pid_file());
    remove_if_exists(port_file());
    Ok(true)
}

/// Save volume (0-100) and push it live to the daemon if running.
/// Returns `(clamped_volume, daemon_was_running)`.
pub fn set_volume(level: i64) -> Result<(u8, bool)> {
    let percent = clamp_percent(level);
    write_saved_volume(i64::from(percent))?;

    if running_pid().is_none() {
        return Ok((percent, false));
    }

    let reply = send_command(&format!("volume {percent}"), Duration::from_secs(2));
    if reply.as_deref().is_some_and(|r| r.starts_with("OK")) {
        Ok((percent, true))
    } else {
        Err(Error::new(format!(
            "volume {percent} saved, but live update failed — try `thocci off && thocci on`"
        )))
    }
}

/// Save active pack id (built-in or user-imported) and swap it live if the daemon is running.
/// Preserves the last working pack when validation or live switching fails.
/// Returns `true` if swapped live, `false` if only saved for next start.
pub fn set_pack(pack_id: &str) -> Result<bool> {
    if !is_known_pack(pack_id) {
        return Err(Error::new(format!(
            "unknown pack '{pack_id}'. Known: {}",
            known_pack_ids().join(", ")
        )));
    }

    if running_pid().is_none() {
        // Validate the pack actually loads before replacing the saved working pack.
        Soundpack::load_by_id(pack_id, None).map_err(|err| {
            Error::new(format!(
                "could not load pack '{pack_id}' ({err}) — keeping previous pack"
            ))
        })?;
        write_saved_pack(pack_id)?;
        return Ok(false);
    }

    let reply = send_command(&format!("pack {pack_id}"), Duration::from_secs(5));
    match reply.as_deref() {
        Some(r) if r.starts_with("OK") => {
            write_saved_pack(pack_id)?;
            Ok(true)
        }
        Some(err_reply) => Err(Error::new(format!(
            "live pack switch to '{pack_id}' failed ({err_reply}) — keeping previous pack"
        ))),
        None => Err(Error::new(format!(
            "live pack switch to '{pack_id}' failed because daemon is unresponsive — keeping previous pack"
        ))),
    }
}

/// Save stereo keystroke panning preference and push it live to the daemon if running.
/// Returns `(enabled, daemon_was_running)`.
///
/// When a daemon is running, only confirms and persists after receiving an `OK`
/// reply so older or unresponsive daemons are never misrepresented as active.
pub fn set_stereo(enabled: bool) -> Result<(bool, bool)> {
    if running_pid().is_none() {
        write_saved_stereo(enabled)?;
        return Ok((enabled, false));
    }

    let arg = if enabled { "on" } else { "off" };
    let reply = send_command(&format!("stereo {arg}"), Duration::from_secs(2));
    match reply.as_deref() {
        Some(r) if r.starts_with("OK") => {
            write_saved_stereo(enabled)?;
            Ok((enabled, true))
        }
        Some(err_reply) => Err(Error::new(format!(
            "stereo audio update was rejected by the running engine ({err_reply}) — restart Thocci to apply"
        ))),
        None => Err(Error::new(
            "stereo audio update failed because the running engine did not respond — restart Thocci and try again",
        )),
    }
}

pub fn list_builtin_packs() -> &'static [BuiltinPackMeta] {
    BUILTIN_PACKS
}

pub fn list_all_packs() -> Vec<PackCatalogEntry> {
    soundpack::list_all_packs()
}

fn parse_status_reply(
    reply: &str,
    pid: u32,
    fallback_volume: u8,
    fallback_pack: &str,
    saved_stereo: bool,
) -> Option<DaemonStatus> {
    let rest = reply.strip_prefix("OK")?.trim();
    let mut volume = fallback_volume;
    let mut pack = fallback_pack.to_owned();
    // An older daemon reply without `stereo=` must not be presented as confirming
    // that stereo panning is active.
    let mut stereo = false;

    for token in rest.split_whitespace() {
        if let Some(val) = token.strip_prefix("volume=") {
            if let Ok(parsed) = val.parse::<u8>() {
                volume = parsed.min(100);
            }
        } else if let Some(val) = token.strip_prefix("pack=") {
            pack = val.to_owned();
        } else if let Some(val) = token.strip_prefix("stereo=") {
            if let Some(parsed) = parse_stereo_flag(val) {
                stereo = parsed;
            }
        }
    }

    Some(DaemonStatus {
        running: true,
        responsive: true,
        pid: Some(pid),
        volume,
        pack,
        stereo,
        saved_volume: fallback_volume,
        saved_pack: fallback_pack.to_owned(),
        saved_stereo,
    })
}

fn read_log_tail(path: &Path, lines: usize) -> String {
    let Ok(content) = fs::read_to_string(path) else {
        return String::new();
    };
    let all: Vec<&str> = content.lines().collect();
    let start = all.len().saturating_sub(lines);
    all[start..]
        .iter()
        .map(|line| format!("  {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::{self, test_support};

    #[test]
    fn failed_pack_switch_preserves_last_working_pack() {
        let _guard = test_support::isolated_state();
        paths::write_saved_pack("creamy").unwrap();

        // Create a broken imported pack directory (valid config.json syntax, missing audio file).
        let broken_dir = paths::soundpacks_dir().join("imported-broken-test");
        std::fs::create_dir_all(&broken_dir).unwrap();
        std::fs::write(
            broken_dir.join("config.json"),
            r#"{"name":"Broken","config_version":"2","definition_method":"single","audio_file":"missing.wav","definitions":{"Generic":{"audio_file":"missing.wav","timing":[[0,50]]}}}"#,
        )
        .unwrap();

        assert!(set_pack("imported-broken-test").is_err());
        assert_eq!(paths::read_saved_pack(), "creamy");

        let _ = std::fs::remove_dir_all(&broken_dir);
    }

    #[test]
    fn parse_status_reply_handles_stereo_and_older_daemons_without_field() {
        // Modern daemon explicitly confirming stereo=on
        let on = parse_status_reply(
            "OK running volume=80 pack=creamy stereo=on",
            42,
            100,
            "cherry-mx",
            true,
        )
        .expect("valid status");
        assert!(on.stereo);
        assert!(on.saved_stereo);
        assert_eq!(on.volume, 80);
        assert_eq!(on.pack, "creamy");

        // Modern daemon confirming stereo=off
        let off = parse_status_reply(
            "OK running volume=80 pack=creamy stereo=off",
            42,
            100,
            "cherry-mx",
            true,
        )
        .expect("valid status");
        assert!(!off.stereo);
        assert!(off.saved_stereo);

        // Older daemon reply omitting `stereo=` still parses cleanly and must NOT claim stereo is active
        let legacy = parse_status_reply(
            "OK running volume=75 pack=banana-split",
            42,
            100,
            "cherry-mx",
            true,
        )
        .expect("legacy status parses");
        assert_eq!(legacy.volume, 75);
        assert_eq!(legacy.pack, "banana-split");
        assert!(!legacy.stereo);
        assert!(legacy.saved_stereo);
    }

    #[test]
    fn set_stereo_persists_when_daemon_is_stopped() {
        let _guard = test_support::isolated_state();
        paths::remove_if_exists(paths::pid_file());
        paths::remove_if_exists(paths::stereo_file());

        let initial = status();
        assert!(!initial.running);
        assert!(!initial.stereo);
        assert!(!initial.saved_stereo);

        assert_eq!(set_stereo(true).unwrap(), (true, false));
        let after_on = status();
        assert!(after_on.stereo);
        assert!(after_on.saved_stereo);

        assert_eq!(set_stereo(false).unwrap(), (false, false));
        let after_off = status();
        assert!(!after_off.stereo);
        assert!(!after_off.saved_stereo);
    }
}
