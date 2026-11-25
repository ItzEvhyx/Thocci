//! Tauri desktop backend for Thocci.
//!
//! Exposes thin IPC commands over `thocci-core` so the future desktop UI shares
//! the exact same daemon, settings, and control protocol as the `thocci` CLI.

use std::path::PathBuf;

use thocci_core::client::{self, DaemonStatus, StartOptions};
use thocci_core::soundpack::BuiltinPackMeta;

#[tauri::command]
fn get_status() -> DaemonStatus {
    client::status()
}

#[tauri::command]
fn list_packs() -> Vec<BuiltinPackMeta> {
    client::list_builtin_packs().to_vec()
}

#[tauri::command]
fn start_daemon(volume: Option<i64>) -> Result<DaemonStatus, String> {
    let exe = resolve_daemon_exe().map_err(|err| err.to_string())?;
    client::start(
        &exe,
        StartOptions {
            pack_dir: None,
            volume,
        },
    )
    .map_err(|err| err.to_string())
}

#[tauri::command]
fn stop_daemon() -> Result<DaemonStatus, String> {
    client::stop().map_err(|err| err.to_string())?;
    Ok(client::status())
}

#[tauri::command]
fn toggle_daemon() -> Result<DaemonStatus, String> {
    if client::running_pid().is_some() {
        stop_daemon()
    } else {
        start_daemon(None)
    }
}

#[tauri::command]
fn set_volume(level: i64) -> Result<DaemonStatus, String> {
    client::set_volume(level).map_err(|err| err.to_string())?;
    Ok(client::status())
}

#[tauri::command]
fn set_pack(pack_id: String) -> Result<DaemonStatus, String> {
    client::set_pack(&pack_id).map_err(|err| err.to_string())?;
    Ok(client::status())
}

/// Prefer a sibling `thocci` CLI binary (lighter process for the background
/// daemon), falling back to this executable (`thocci-app daemon ...`).
fn resolve_daemon_exe() -> std::io::Result<PathBuf> {
    let current = std::env::current_exe()?;
    let bin_name = if cfg!(windows) {
        "thocci.exe"
    } else {
        "thocci"
    };
    if let Some(dir) = current.parent() {
        let sibling = dir.join(bin_name);
        if sibling.is_file() {
            return Ok(sibling);
        }
    }
    Ok(current)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            get_status,
            list_packs,
            start_daemon,
            stop_daemon,
            toggle_daemon,
            set_volume,
            set_pack,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Thocci Tauri application");
}
