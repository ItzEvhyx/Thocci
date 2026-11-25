//! High-level daemon client shared by the `thocci` CLI and the Tauri app.

use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use serde::Serialize;

use crate::control::send_command;
use crate::paths::{
    clamp_percent, ensure_state_dir, log_file, pid_file, port_file, read_pid_file, read_saved_pack,
    read_saved_volume, remove_if_exists, write_saved_pack, write_saved_volume,
};
use crate::process::{pid_is_running, spawn_detached, terminate_pid};
use crate::soundpack::{is_builtin, known_pack_ids, BuiltinPackMeta, BUILTIN_PACKS};
use crate::{Error, Result};

/// Snapshot of daemon status for both CLI display and Tauri UI state.
#[derive(Debug, Clone, Serialize)]
pub struct DaemonStatus {
    pub running: bool,
    pub pid: Option<u32>,
    pub volume: u8,
    pub pack: String,
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
    let Some(pid) = running_pid() else {
        return DaemonStatus {
            running: false,
            pid: None,
            volume: saved_volume,
            pack: saved_pack,
        };
    };

    if let Some(reply) = send_command("status", Duration::from_secs(1)) {
        if let Some(parsed) = parse_status_reply(&reply, pid, saved_volume, &saved_pack) {
            return parsed;
        }
    }

    DaemonStatus {
        running: true,
        pid: Some(pid),
        volume: saved_volume,
        pack: saved_pack,
    }
}

/// Start the background daemon using `daemon_exe` (normally the `thocci` CLI binary).
pub fn start(daemon_exe: &Path, options: StartOptions) -> Result<DaemonStatus> {
    if running_pid().is_some() {
        return Ok(status());
    }

    ensure_state_dir()?;
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

    for _ in 0..100 {
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
                    return Ok(status());
                }
            }
        }
    }

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
        return Ok(false);
    };

    if send_command("stop", Duration::from_secs(1)).is_none() {
        terminate_pid(pid, false)?;
    }

    for _ in 0..40 {
        thread::sleep(Duration::from_millis(50));
        if running_pid().is_none() {
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

/// Save active built-in pack id and swap it live if the daemon is running.
/// Returns `true` if swapped live, `false` if only saved for next start.
pub fn set_pack(pack_id: &str) -> Result<bool> {
    if !is_builtin(pack_id) {
        return Err(Error::new(format!(
            "unknown pack '{pack_id}'. Known: {}",
            known_pack_ids().join(", ")
        )));
    }
    write_saved_pack(pack_id)?;

    if running_pid().is_none() {
        return Ok(false);
    }

    let reply = send_command(&format!("pack {pack_id}"), Duration::from_secs(5));
    if reply.as_deref().is_some_and(|r| r.starts_with("OK")) {
        Ok(true)
    } else {
        Err(Error::new(format!(
            "pack {pack_id} saved, but live switch failed — try `thocci off && thocci on`"
        )))
    }
}

pub fn list_builtin_packs() -> &'static [BuiltinPackMeta] {
    BUILTIN_PACKS
}

fn parse_status_reply(
    reply: &str,
    pid: u32,
    fallback_volume: u8,
    fallback_pack: &str,
) -> Option<DaemonStatus> {
    let rest = reply.strip_prefix("OK")?.trim();
    let mut volume = fallback_volume;
    let mut pack = fallback_pack.to_owned();

    for token in rest.split_whitespace() {
        if let Some(val) = token.strip_prefix("volume=") {
            if let Ok(parsed) = val.parse::<u8>() {
                volume = parsed.min(100);
            }
        } else if let Some(val) = token.strip_prefix("pack=") {
            pack = val.to_owned();
        }
    }

    Some(DaemonStatus {
        running: true,
        pid: Some(pid),
        volume,
        pack,
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
