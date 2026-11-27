//! Tauri desktop backend for Thocci.
//!
//! Exposes serialized, off-UI-thread IPC commands over `thocci-core` so the
//! desktop dashboard, Quick Actions popover, and `thocci` CLI share one daemon,
//! one pack catalog, and one volume setting while keeping UI preferences separate.

#[cfg(target_os = "linux")]
mod layer_shell;
mod platform;
mod prefs;
mod preview;
mod tray;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;
#[cfg(not(target_os = "linux"))]
use tauri::menu::MenuItem;
use tauri::{AppHandle, Emitter, LogicalSize, Manager, WindowEvent, Wry};
use tauri_plugin_dialog::DialogExt;
use thocci_core::client::{self, DaemonStatus, StartOptions};
use thocci_core::soundpack::{self, BuiltinPackMeta, PackCatalogEntry};

use crate::platform::PlatformInfo;
use crate::prefs::UiPrefs;
use crate::preview::PreviewPlayer;

const ONBOARDING_WIDTH: f64 = 480.0;
const ONBOARDING_HEIGHT: f64 = 474.0;

#[cfg(not(target_os = "linux"))]
#[derive(Debug, Clone, Serialize)]
pub struct OverlayPlacement {
    #[serde(rename = "layerShell")]
    pub layer_shell: bool,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    #[serde(rename = "screenWidth")]
    pub screen_width: i32,
    #[serde(rename = "screenHeight")]
    pub screen_height: i32,
}

#[cfg(target_os = "linux")]
pub use layer_shell::OverlayPlacement;

#[cfg(not(target_os = "linux"))]
#[derive(Debug, Clone, Serialize)]
pub struct QuickActionsPlacement {
    #[serde(rename = "layerShell")]
    pub layer_shell: bool,
    pub x: i32,
    pub y: i32,
    #[serde(rename = "screenWidth")]
    pub screen_width: i32,
    #[serde(rename = "screenHeight")]
    pub screen_height: i32,
}

#[cfg(target_os = "linux")]
pub use layer_shell::QuickActionsPlacement;

pub(crate) struct AppState {
    pub engine_lock: Arc<Mutex<()>>,
    pub preview: Arc<PreviewPlayer>,
    pub tray_available: AtomicBool,
    #[cfg(target_os = "linux")]
    pub linux_tray_handle: Mutex<Option<ksni::Handle<tray::LinuxTray>>>,
    #[cfg(not(target_os = "linux"))]
    pub tray_toggle_item: Mutex<Option<MenuItem<Wry>>>,
    pub last_quick_actions_blur_ms: AtomicU64,
    pub last_quick_actions_show_ms: AtomicU64,
    pub last_quick_actions_placement: Mutex<Option<QuickActionsPlacement>>,
    pub is_quitting: AtomicBool,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            engine_lock: Arc::new(Mutex::new(())),
            preview: Arc::new(PreviewPlayer::new()),
            tray_available: AtomicBool::new(false),
            #[cfg(target_os = "linux")]
            linux_tray_handle: Mutex::new(None),
            #[cfg(not(target_os = "linux"))]
            tray_toggle_item: Mutex::new(None),
            last_quick_actions_blur_ms: AtomicU64::new(0),
            last_quick_actions_show_ms: AtomicU64::new(0),
            last_quick_actions_placement: Mutex::new(None),
            is_quitting: AtomicBool::new(false),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportResult {
    pub imported: PackCatalogEntry,
    pub catalog: Vec<PackCatalogEntry>,
    pub status: DaemonStatus,
}

#[derive(Debug, Clone, Serialize)]
struct NavigatePayload {
    view: String,
}

#[tauri::command]
async fn get_status(app: AppHandle<Wry>) -> Result<DaemonStatus, String> {
    let status = tauri::async_runtime::spawn_blocking(client::status)
        .await
        .map_err(|err| err.to_string())?;
    tray::refresh_tray_status(&app, status.running);
    Ok(status)
}

#[tauri::command]
fn list_packs() -> Vec<BuiltinPackMeta> {
    client::list_builtin_packs().to_vec()
}

#[tauri::command]
async fn list_pack_catalog() -> Result<Vec<PackCatalogEntry>, String> {
    tauri::async_runtime::spawn_blocking(client::list_all_packs)
        .await
        .map_err(|err| err.to_string())
}

#[tauri::command]
async fn start_daemon(app: AppHandle<Wry>, volume: Option<i64>) -> Result<DaemonStatus, String> {
    let lock = Arc::clone(&app.state::<AppState>().engine_lock);
    let status = tauri::async_runtime::spawn_blocking(move || {
        let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());
        let exe = resolve_daemon_exe().map_err(|err| err.to_string())?;
        client::start(
            &exe,
            StartOptions {
                pack_dir: None,
                volume,
            },
        )
        .map_err(|err| err.to_string())
    })
    .await
    .map_err(|err| err.to_string())??;

    broadcast_status(&app, &status);
    Ok(status)
}

#[tauri::command]
async fn stop_daemon(app: AppHandle<Wry>) -> Result<DaemonStatus, String> {
    let lock = Arc::clone(&app.state::<AppState>().engine_lock);
    let status = tauri::async_runtime::spawn_blocking(move || {
        let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());
        client::stop().map_err(|err| err.to_string())?;
        Ok::<DaemonStatus, String>(client::status())
    })
    .await
    .map_err(|err| err.to_string())??;

    broadcast_status(&app, &status);
    Ok(status)
}

#[tauri::command]
async fn toggle_daemon(app: AppHandle<Wry>) -> Result<DaemonStatus, String> {
    toggle_daemon_internal(app).await
}

pub(crate) async fn toggle_daemon_internal(app: AppHandle<Wry>) -> Result<DaemonStatus, String> {
    let lock = Arc::clone(&app.state::<AppState>().engine_lock);
    let status = tauri::async_runtime::spawn_blocking(move || {
        let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());
        if client::running_pid().is_some() {
            client::stop().map_err(|err| err.to_string())?;
            Ok::<DaemonStatus, String>(client::status())
        } else {
            let exe = resolve_daemon_exe().map_err(|err| err.to_string())?;
            client::start(
                &exe,
                StartOptions {
                    pack_dir: None,
                    volume: None,
                },
            )
            .map_err(|err| err.to_string())
        }
    })
    .await
    .map_err(|err| err.to_string())??;

    broadcast_status(&app, &status);
    Ok(status)
}

#[tauri::command]
async fn set_volume(app: AppHandle<Wry>, level: i64) -> Result<DaemonStatus, String> {
    let lock = Arc::clone(&app.state::<AppState>().engine_lock);
    let result = tauri::async_runtime::spawn_blocking(move || {
        let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());
        let res = client::set_volume(level).map_err(|err| err.to_string());
        let st = client::status();
        (res, st)
    })
    .await
    .map_err(|err| err.to_string())?;

    broadcast_status(&app, &result.1);
    result.0.map(|_| result.1)
}

#[tauri::command]
async fn set_pack(app: AppHandle<Wry>, pack_id: String) -> Result<DaemonStatus, String> {
    let state = app.state::<AppState>();
    let lock = Arc::clone(&state.engine_lock);
    let preview = Arc::clone(&state.preview);
    let result = tauri::async_runtime::spawn_blocking(move || {
        let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());
        let res = client::set_pack(&pack_id).map_err(|err| err.to_string());
        if res.is_ok() {
            preview.invalidate_cache();
        }
        let st = client::status();
        (res, st)
    })
    .await
    .map_err(|err| err.to_string())?;

    broadcast_status(&app, &result.1);
    result.0.map(|_| result.1)
}

#[tauri::command]
async fn import_wav_pack(
    app: AppHandle<Wry>,
    path: String,
    name: Option<String>,
) -> Result<ImportResult, String> {
    import_wav_from_path(app, PathBuf::from(path), name).await
}

#[tauri::command]
async fn pick_and_import_wav(app: AppHandle<Wry>) -> Result<Option<ImportResult>, String> {
    #[cfg(target_os = "linux")]
    let demoted_layer = if layer_shell::is_active() {
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let _ = app.run_on_main_thread(move || {
            layer_shell::set_panel_modal_dialog_open("main", true);
            let _ = ready_tx.send(());
        });
        let _ = ready_rx.recv_timeout(std::time::Duration::from_millis(250));
        true
    } else {
        false
    };
    #[cfg(not(target_os = "linux"))]
    let demoted_layer = false;

    let _ = app.emit_to("main", "thocci://file-dialog-state", true);

    let (tx, rx) = std::sync::mpsc::channel();
    let mut builder = app
        .dialog()
        .file()
        .set_title("Import WAV soundpack")
        .add_filter("WAV Audio (*.wav)", &["wav", "WAV"]);

    // On macOS, Windows, and non-layer-shell Linux sessions, attach the main
    // window as modal parent so the picker stays strictly above Thocci.
    if !demoted_layer {
        if let Some(main_win) = app.get_webview_window("main") {
            builder = builder.set_parent(&main_win);
        }
    }

    builder.pick_file(move |file_path| {
        let _ = tx.send(file_path);
    });

    let picked = tauri::async_runtime::spawn_blocking(move || rx.recv().ok().flatten())
        .await
        .map_err(|err| err.to_string());

    #[cfg(target_os = "linux")]
    if demoted_layer {
        let app_restore = app.clone();
        let _ = app.run_on_main_thread(move || {
            layer_shell::set_panel_modal_dialog_open("main", false);
            if let Some(main_win) = app_restore.get_webview_window("main") {
                let _ = main_win.set_focus();
            }
        });
    }
    let _ = app.emit_to("main", "thocci://file-dialog-state", false);

    let Some(file_path) = picked? else {
        return Ok(None);
    };
    let path = file_path
        .into_path()
        .map_err(|err| format!("Selected file is not a local path: {err}"))?;

    let imported = import_wav_from_path(app, path, None).await?;
    Ok(Some(imported))
}

async fn import_wav_from_path(
    app: AppHandle<Wry>,
    path: PathBuf,
    name: Option<String>,
) -> Result<ImportResult, String> {
    let lock = Arc::clone(&app.state::<AppState>().engine_lock);
    let result = tauri::async_runtime::spawn_blocking(move || {
        let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());
        let imported =
            soundpack::import_wav(&path, name.as_deref()).map_err(|err| err.to_string())?;
        let catalog = client::list_all_packs();
        let status = client::status();
        Ok::<ImportResult, String>(ImportResult {
            imported,
            catalog,
            status,
        })
    })
    .await
    .map_err(|err| err.to_string())??;

    let _ = app.emit("thocci://catalog-changed", &result.catalog);
    broadcast_status(&app, &result.status);
    Ok(result)
}

#[tauri::command]
async fn play_preview(
    app: AppHandle<Wry>,
    code: Option<String>,
    kind: Option<String>,
) -> Result<bool, String> {
    let preview = Arc::clone(&app.state::<AppState>().preview);
    let kind_str = kind.unwrap_or_else(|| "down".to_owned());
    tauri::async_runtime::spawn_blocking(move || {
        let st = client::status();
        preview.play(code.as_deref(), &kind_str, &st.pack, st.volume)
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command]
async fn get_ui_prefs() -> Result<UiPrefs, String> {
    tauri::async_runtime::spawn_blocking(prefs::load_prefs)
        .await
        .map_err(|err| err.to_string())
}

#[tauri::command]
async fn save_ui_prefs(app: AppHandle<Wry>, prefs_update: UiPrefs) -> Result<UiPrefs, String> {
    let saved = tauri::async_runtime::spawn_blocking(move || prefs::save_prefs(&prefs_update))
        .await
        .map_err(|err| err.to_string())??;
    let _ = app.emit("thocci://prefs-changed", &saved);
    Ok(saved)
}

#[tauri::command]
async fn set_window_size(
    app: AppHandle<Wry>,
    width: Option<u32>,
    height: Option<u32>,
) -> Result<UiPrefs, String> {
    let saved = tauri::async_runtime::spawn_blocking(move || {
        let mut current = prefs::load_prefs();
        current.window_width = width;
        current.window_height = height;
        prefs::save_prefs(&current)
    })
    .await
    .map_err(|err| err.to_string())??;

    let (w, h) = saved.effective_dashboard_size();
    #[cfg(target_os = "linux")]
    let handled_by_layer = if layer_shell::is_active() {
        let app_clone = app.clone();
        let _ = app_clone.run_on_main_thread(move || {
            layer_shell::center_panel("main", w as i32, h as i32);
        });
        true
    } else {
        false
    };
    #[cfg(not(target_os = "linux"))]
    let handled_by_layer = false;

    if !handled_by_layer {
        if let Some(win) = app.get_webview_window("main") {
            let _ = win.set_size(LogicalSize::new(f64::from(w), f64::from(h)));
            let _ = win.center();
        }
        platform::ensure_floating_overlay("Thocci", w, h, true, false);
    }
    let _ = app.emit("thocci://prefs-changed", &saved);
    Ok(saved)
}

#[tauri::command]
fn get_overlay_placement(app: AppHandle<Wry>) -> Result<Option<OverlayPlacement>, String> {
    #[cfg(target_os = "linux")]
    if layer_shell::is_active() {
        let (tx, rx) = std::sync::mpsc::channel();
        app.run_on_main_thread(move || {
            let _ = tx.send(layer_shell::get_panel_placement("main"));
        })
        .map_err(|e| e.to_string())?;
        return rx
            .recv_timeout(std::time::Duration::from_millis(500))
            .map_err(|e| e.to_string());
    }
    let _ = app;
    Ok(None)
}

#[tauri::command]
fn set_overlay_input_fullscreen(app: AppHandle<Wry>, fullscreen: bool) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    if layer_shell::is_active() {
        app.run_on_main_thread(move || {
            layer_shell::set_panel_input_fullscreen("main", fullscreen);
        })
        .map_err(|e| e.to_string())?;
    }
    #[cfg(not(target_os = "linux"))]
    let _ = (app, fullscreen);
    Ok(())
}

#[tauri::command]
async fn set_overlay_rect(
    app: AppHandle<Wry>,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    persist_size: Option<bool>,
) -> Result<(), String> {
    let clamped_w = width.clamp(
        prefs::MIN_DASHBOARD_WIDTH as i32,
        prefs::MAX_DASHBOARD_WIDTH as i32,
    );
    let clamped_h = height.clamp(
        prefs::MIN_DASHBOARD_HEIGHT as i32,
        prefs::MAX_DASHBOARD_HEIGHT as i32,
    );

    #[cfg(target_os = "linux")]
    if layer_shell::is_active() {
        let clamped_x = x.clamp(-clamped_w + 120, 7680);
        let clamped_y = y.clamp(0, 4320);
        let _ = app.run_on_main_thread(move || {
            layer_shell::set_panel_rect(
                "main",
                layer_shell::PanelRect {
                    x: clamped_x,
                    y: clamped_y,
                    width: clamped_w,
                    height: clamped_h,
                },
                false,
            );
        });
    }
    #[cfg(not(target_os = "linux"))]
    let _ = (x, y);

    if persist_size.unwrap_or(false) {
        let saved = tauri::async_runtime::spawn_blocking(move || {
            let mut current = prefs::load_prefs();
            current.window_width = Some(clamped_w as u32);
            current.window_height = Some(clamped_h as u32);
            prefs::save_prefs(&current)
        })
        .await
        .map_err(|e| e.to_string())??;
        let _ = app.emit("thocci://prefs-changed", &saved);
    }

    Ok(())
}

#[tauri::command]
fn get_quick_actions_placement(app: AppHandle<Wry>) -> Option<QuickActionsPlacement> {
    app.state::<AppState>()
        .last_quick_actions_placement
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

#[tauri::command]
async fn complete_onboarding(app: AppHandle<Wry>) -> Result<UiPrefs, String> {
    let saved = tauri::async_runtime::spawn_blocking(|| {
        let mut current = prefs::load_prefs();
        current.onboarding_completed = true;
        prefs::save_prefs(&current)
    })
    .await
    .map_err(|err| err.to_string())??;

    configure_main_window_for_mode(&app, true, &saved);
    ensure_tray_initialized_async(&app).await;
    let _ = app.emit("thocci://prefs-changed", &saved);
    Ok(saved)
}

#[tauri::command]
async fn get_platform_info(app: AppHandle<Wry>) -> Result<PlatformInfo, String> {
    let tray_available = app
        .state::<AppState>()
        .tray_available
        .load(Ordering::Acquire);
    tauri::async_runtime::spawn_blocking(move || {
        let exe = resolve_daemon_exe().unwrap_or_else(|_| PathBuf::from("thocci"));
        platform::collect_platform_info(tray_available, &exe)
    })
    .await
    .map_err(|err| err.to_string())
}

#[tauri::command]
fn open_dashboard(app: AppHandle<Wry>, route: Option<String>) -> Result<(), String> {
    show_dashboard_window(&app, route)
}

#[tauri::command]
fn open_quick_actions(app: AppHandle<Wry>) -> Result<(), String> {
    tray::show_quick_actions_window(&app, None);
    Ok(())
}

#[tauri::command]
fn hide_quick_actions(app: AppHandle<Wry>) -> Result<(), String> {
    tray::hide_quick_actions_window(&app);
    Ok(())
}

#[tauri::command]
async fn quit_app(app: AppHandle<Wry>) -> Result<(), String> {
    shutdown_and_exit(app).await;
    Ok(())
}

pub(crate) async fn shutdown_and_exit(app: AppHandle<Wry>) {
    let state = app.state::<AppState>();
    state.is_quitting.store(true, Ordering::Release);
    tray::shutdown_tray(&app).await;
    let lock = Arc::clone(&state.engine_lock);
    let preview = Arc::clone(&state.preview);

    let _ = tauri::async_runtime::spawn_blocking(move || {
        let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());
        let _ = client::stop();
        preview.shutdown();
    })
    .await;

    app.exit(0);
}

pub(crate) fn show_dashboard_window(
    app: &AppHandle<Wry>,
    route: Option<String>,
) -> Result<(), String> {
    tray::hide_quick_actions_window(app);

    let Some(win) = app.get_webview_window("main") else {
        return Err("Main dashboard window is unavailable".to_owned());
    };

    let _ = win.show();
    let _ = win.unminimize();
    let _ = win.set_focus();

    if let Some(view) = route {
        let _ = app.emit_to("main", "thocci://navigate", NavigatePayload { view });
    }
    let _ = app.emit_to("main", "thocci://dashboard-shown", ());
    Ok(())
}

fn fitted_dashboard_size_for_window(
    win: &tauri::WebviewWindow<Wry>,
    desired_w: u32,
    desired_h: u32,
) -> (u32, u32) {
    let monitor = win
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| win.primary_monitor().ok().flatten());
    if let Some(m) = monitor {
        let scale = m.scale_factor().max(0.5);
        let wa = m.work_area();
        let work_w = (f64::from(wa.size.width) / scale).round() as u32;
        let work_h = (f64::from(wa.size.height) / scale).round() as u32;
        return prefs::fit_dashboard_size_to_work_area(desired_w, desired_h, work_w, work_h);
    }
    (desired_w, desired_h)
}

fn configure_main_window_for_mode(app: &AppHandle<Wry>, dashboard_mode: bool, ui_prefs: &UiPrefs) {
    let Some(win) = app.get_webview_window("main") else {
        return;
    };

    let (desired_w, desired_h) = if dashboard_mode {
        ui_prefs.effective_dashboard_size()
    } else {
        (ONBOARDING_WIDTH as u32, ONBOARDING_HEIGHT as u32)
    };

    #[cfg(target_os = "linux")]
    if layer_shell::is_active() {
        let (mon_w, mon_h) = layer_shell::current_monitor_logical_size().unwrap_or((1920, 1080));
        let (target_w, target_h) = if dashboard_mode {
            prefs::fit_dashboard_size_to_work_area(
                desired_w,
                desired_h,
                mon_w.max(320) as u32,
                mon_h.max(320) as u32,
            )
        } else {
            (desired_w, desired_h)
        };
        let _ = app.run_on_main_thread(move || {
            layer_shell::center_panel("main", target_w as i32, target_h as i32);
        });
        return;
    }

    if dashboard_mode {
        let (target_w, target_h) = fitted_dashboard_size_for_window(&win, desired_w, desired_h);
        let _ = win.set_resizable(true);
        let _ = win.set_min_size(Some(LogicalSize::new(
            f64::from(prefs::MIN_DASHBOARD_WIDTH),
            f64::from(prefs::MIN_DASHBOARD_HEIGHT),
        )));
        let _ = win.set_size(LogicalSize::new(f64::from(target_w), f64::from(target_h)));
        let _ = win.center();
        platform::ensure_floating_overlay("Thocci", target_w, target_h, true, false);
    } else {
        let _ = win.set_min_size(Some(LogicalSize::new(ONBOARDING_WIDTH, ONBOARDING_HEIGHT)));
        let _ = win.set_size(LogicalSize::new(ONBOARDING_WIDTH, ONBOARDING_HEIGHT));
        let _ = win.set_resizable(false);
        let _ = win.center();
        platform::ensure_floating_overlay("Thocci", desired_w, desired_h, true, false);
    }
}

fn ensure_tray_initialized(app: &AppHandle<Wry>) {
    let state = app.state::<AppState>();
    if state.tray_available.load(Ordering::Acquire) {
        return;
    }
    let running = client::running_pid().is_some();
    if tray::setup_tray(app, running) {
        state.tray_available.store(true, Ordering::Release);
    }
}

async fn ensure_tray_initialized_async(app: &AppHandle<Wry>) {
    let state = app.state::<AppState>();
    if state.tray_available.load(Ordering::Acquire) {
        return;
    }
    let running = client::running_pid().is_some();
    if tray::setup_tray_async(app, running).await {
        state.tray_available.store(true, Ordering::Release);
    }
}

fn broadcast_status(app: &AppHandle<Wry>, status: &DaemonStatus) {
    tray::refresh_tray_status(app, status.running);
    let _ = app.emit("thocci://state-changed", status);
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
    #[cfg(target_os = "linux")]
    layer_shell::silence_ayatana_deprecation_warning();

    tauri::Builder::default()
        .manage(AppState::default())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if args.iter().any(|arg| arg == "--quick-actions") {
                tray::toggle_quick_actions_anchored(app, None);
                return;
            }
            let route = if args.iter().any(|arg| arg == "--soundpacks") {
                Some("soundpacks".to_owned())
            } else {
                None
            };
            let _ = show_dashboard_window(app, route);
        }))
        .setup(|app| {
            let handle = app.handle();
            let ui_prefs = prefs::load_prefs();
            let (init_w, init_h) = if ui_prefs.onboarding_completed {
                ui_prefs.effective_dashboard_size()
            } else {
                (ONBOARDING_WIDTH as u32, ONBOARDING_HEIGHT as u32)
            };

            #[cfg(target_os = "linux")]
            let layer_enabled = {
                let (mon_w, mon_h) =
                    layer_shell::current_monitor_logical_size().unwrap_or((1920, 1080));
                let (fit_w, fit_h) = if ui_prefs.onboarding_completed {
                    prefs::fit_dashboard_size_to_work_area(
                        init_w,
                        init_h,
                        mon_w.max(320) as u32,
                        mon_h.max(320) as u32,
                    )
                } else {
                    (init_w, init_h)
                };
                let main_rect = layer_shell::PanelRect {
                    x: ((mon_w - fit_w as i32) / 2).max(0),
                    y: ((mon_h - fit_h as i32) / 2).max(0),
                    width: fit_w as i32,
                    height: fit_h as i32,
                };
                app.get_webview_window("main")
                    .map(|w| {
                        layer_shell::setup_window(
                            handle,
                            &w,
                            "thocci-dashboard",
                            layer_shell::LayerKind::FloatingPanel,
                            main_rect,
                        )
                    })
                    .unwrap_or(false)
            };
            #[cfg(not(target_os = "linux"))]
            let layer_enabled = false;

            if !layer_enabled {
                // On macOS, Windows, and non-layer-shell Linux sessions (X11 / GNOME),
                // give the native floating desktop window a clean OS shadow on macOS/Windows.
                #[cfg(not(target_os = "linux"))]
                if let Some(main_win) = app.get_webview_window("main") {
                    let _ = main_win.set_shadow(true);
                }
                platform::register_hyprland_window_rules(init_w, init_h);
            }

            let launch_quick_actions =
                ui_prefs.onboarding_completed && std::env::args().any(|a| a == "--quick-actions");

            if ui_prefs.onboarding_completed {
                ensure_tray_initialized(handle);
                configure_main_window_for_mode(handle, true, &ui_prefs);
            } else {
                configure_main_window_for_mode(handle, false, &ui_prefs);
            }

            if launch_quick_actions {
                tray::show_quick_actions_window(handle, None);
            } else if let Some(main_win) = app.get_webview_window("main") {
                let _ = main_win.show();
                let _ = main_win.set_focus();
                if !layer_enabled {
                    platform::ensure_floating_overlay("Thocci", init_w, init_h, true, false);
                }
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            let label = window.label();
            match (label, event) {
                ("quick-actions", WindowEvent::Focused(false)) => {
                    let app = window.app_handle();
                    let state = app.state::<AppState>();
                    let now = tray::now_ms();
                    let last_show = state.last_quick_actions_show_ms.load(Ordering::Acquire);
                    // Ignore transient focus loss while the WM floats/anchors the popup.
                    if now.saturating_sub(last_show) < 350 {
                        return;
                    }
                    state
                        .last_quick_actions_blur_ms
                        .store(now, Ordering::Release);
                    tray::hide_quick_actions_window(app);
                }
                ("quick-actions", WindowEvent::CloseRequested { api, .. }) => {
                    api.prevent_close();
                    tray::hide_quick_actions_window(window.app_handle());
                }
                ("main", WindowEvent::CloseRequested { api, .. }) => {
                    let app = window.app_handle();
                    let state = app.state::<AppState>();
                    let quitting = state.is_quitting.load(Ordering::Acquire);
                    let has_tray = state.tray_available.load(Ordering::Acquire);
                    if !quitting && has_tray {
                        api.prevent_close();
                        let _ = app.emit_to("main", "thocci://dashboard-hidden", ());
                        #[cfg(target_os = "linux")]
                        if layer_shell::is_active() {
                            let _ = app.run_on_main_thread(|| {
                                layer_shell::hide_window("main");
                            });
                        } else {
                            let _ = window.hide();
                        }
                        #[cfg(not(target_os = "linux"))]
                        let _ = window.hide();
                    }
                }
                _ => {}
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_status,
            list_packs,
            list_pack_catalog,
            start_daemon,
            stop_daemon,
            toggle_daemon,
            set_volume,
            set_pack,
            import_wav_pack,
            pick_and_import_wav,
            play_preview,
            get_ui_prefs,
            save_ui_prefs,
            set_window_size,
            get_overlay_placement,
            set_overlay_input_fullscreen,
            set_overlay_rect,
            get_quick_actions_placement,
            complete_onboarding,
            get_platform_info,
            open_dashboard,
            open_quick_actions,
            hide_quick_actions,
            quit_app,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Thocci Tauri application");
}
