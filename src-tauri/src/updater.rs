//! Cross-platform signed in-app updater backed by `tauri-plugin-updater`.
//!
//! Checks `https://github.com/ItzEvhyx/Thocci/releases/latest/download/latest.json`,
//! verifies Ed25519 signatures before writing anything to disk, shuts down the
//! background sound daemon and system tray cleanly prior to replacement, and
//! restarts Thocci into the newly installed version.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Wry};
use tauri_plugin_updater::UpdaterExt;
use thocci_core::client;

use crate::AppState;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub available: bool,
    pub current_version: String,
    pub version: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProgress {
    pub phase: &'static str,
    pub downloaded: u64,
    pub total: Option<u64>,
}

pub async fn check_for_update(app: &AppHandle<Wry>) -> Result<UpdateInfo, String> {
    let current_version = app.package_info().version.to_string();
    let updater = app
        .updater()
        .map_err(|err| format!("Updater is unavailable: {err}"))?;

    match updater.check().await {
        Ok(Some(update)) => {
            let info = UpdateInfo {
                available: true,
                current_version: update.current_version.clone(),
                version: Some(update.version.clone()),
                notes: update.body.clone(),
            };
            let state = app.state::<AppState>();
            *state
                .pending_update
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = Some(update);
            Ok(info)
        }
        Ok(None) => {
            let state = app.state::<AppState>();
            *state
                .pending_update
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = None;
            Ok(UpdateInfo {
                available: false,
                current_version,
                version: None,
                notes: None,
            })
        }
        Err(err) => Err(format!("Could not check for updates: {err}")),
    }
}

pub async fn install_pending_update(app: AppHandle<Wry>) -> Result<(), String> {
    let cached = app
        .state::<AppState>()
        .pending_update
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take();

    let update = match cached {
        Some(u) => u,
        None => {
            let updater = app
                .updater()
                .map_err(|err| format!("Updater is unavailable: {err}"))?;
            updater
                .check()
                .await
                .map_err(|err| format!("Could not check for updates: {err}"))?
                .ok_or_else(|| "Thocci is already up to date.".to_owned())?
        }
    };

    let mut downloaded: u64 = 0;
    let mut last_emitted_bucket: u64 = 0;
    let progress_app = app.clone();
    let finish_app = app.clone();

    let bytes = update
        .download(
            move |chunk_len, content_length| {
                downloaded = downloaded.saturating_add(chunk_len as u64);
                let bucket = downloaded / (64 * 1024);
                if bucket != last_emitted_bucket
                    || content_length.is_some_and(|total| downloaded >= total)
                {
                    last_emitted_bucket = bucket;
                    let _ = progress_app.emit(
                        "thocci://update-progress",
                        UpdateProgress {
                            phase: "downloading",
                            downloaded,
                            total: content_length,
                        },
                    );
                }
            },
            move || {
                let _ = finish_app.emit(
                    "thocci://update-progress",
                    UpdateProgress {
                        phase: "installing",
                        downloaded: 0,
                        total: None,
                    },
                );
            },
        )
        .await
        .map_err(|err| format!("Update download failed: {err}"))?;

    // Release daemon locks, preview audio, and tray registration cleanly
    // before replacing the executable or running the platform installer.
    {
        let state = app.state::<AppState>();
        state.is_quitting.store(true, Ordering::Release);
        crate::tray::shutdown_tray(&app).await;
        let lock = Arc::clone(&state.engine_lock);
        let preview = Arc::clone(&state.preview);
        let _ = tauri::async_runtime::spawn_blocking(move || {
            let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());
            let _ = client::stop();
            preview.shutdown();
        })
        .await;
    }

    if let Err(err) = update.install(bytes) {
        app.state::<AppState>()
            .is_quitting
            .store(false, Ordering::Release);
        return Err(format!("Update installation failed: {err}"));
    }

    // On Windows, `update.install` exits after launching NSIS.
    // On macOS and Linux, restart directly into the newly installed binary.
    app.restart();
}
