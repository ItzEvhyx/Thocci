//! Cross-platform menu-bar / system-tray host and Quick Actions popover routing.

use std::sync::atomic::Ordering;
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, Rect, WebviewUrl,
    WebviewWindowBuilder, Wry,
};

use crate::platform::{compute_popover_position, fallback_popover_position, PixelRect};
use crate::AppState;

const TRAY_ID: &str = "thocci-tray";
const MENU_TOGGLE: &str = "tray-toggle";
const MENU_QUICK_ACTIONS: &str = "tray-quick-actions";
const MENU_DASHBOARD: &str = "tray-dashboard";
const MENU_SOUNDPACKS: &str = "tray-soundpacks";
const MENU_QUIT: &str = "tray-quit";

const QUICK_ACTIONS_LOGICAL_W: f64 = 288.0;
const QUICK_ACTIONS_LOGICAL_H: f64 = 282.0;

#[cfg(target_os = "macos")]
const TRAY_ICON_PNG: &[u8] =
    include_bytes!("../../assets/desktop/brand/tray/thocci-tray-template-32.png");

#[cfg(not(target_os = "macos"))]
const TRAY_ICON_PNG: &[u8] =
    include_bytes!("../../assets/desktop/brand/tray/thocci-tray-light-32.png");

#[cfg(target_os = "linux")]
const LINUX_APPINDICATOR_SONAMES: &[&std::ffi::CStr] = &[
    c"libayatana-appindicator3.so.1",
    c"libappindicator3.so.1",
    c"libayatana-appindicator3.so",
    c"libappindicator3.so",
];

#[cfg(target_os = "linux")]
fn can_dlopen_any(sonames: &[&std::ffi::CStr]) -> bool {
    for soname in sonames {
        // SAFETY: `soname` is a valid NUL-terminated C string; any non-null
        // handle returned by `dlopen` is immediately released with `dlclose`.
        let handle = unsafe { libc::dlopen(soname.as_ptr(), libc::RTLD_LAZY | libc::RTLD_LOCAL) };
        if !handle.is_null() {
            unsafe {
                libc::dlclose(handle);
            }
            return true;
        }
    }
    false
}

/// Probe whether any `libappindicator3` / `libayatana-appindicator3` shared
/// library can be loaded before invoking Tauri's tray builder.
///
/// On Linux, `tray-icon` uses `libappindicator-sys`, which panics inside its
/// lazy library loader if none of these libraries are installed on the host.
/// Checking with `dlopen` first lets Thocci fall back cleanly to dashboard mode
/// instead of crashing on startup on minimal Linux / Arch / Hyprland setups.
#[cfg(target_os = "linux")]
fn has_linux_appindicator_lib() -> bool {
    can_dlopen_any(LINUX_APPINDICATOR_SONAMES)
}

pub fn setup_tray(app: &AppHandle<Wry>, initial_running: bool) -> bool {
    #[cfg(target_os = "linux")]
    if !has_linux_appindicator_lib() {
        log::warn!(
            "System tray library (libayatana-appindicator3 / libappindicator3) not found; continuing with dashboard fallback"
        );
        return false;
    }

    let toggle_label = if initial_running {
        "Disable Thocci"
    } else {
        "Enable Thocci"
    };

    let Ok(toggle_item) = MenuItem::with_id(app, MENU_TOGGLE, toggle_label, true, None::<&str>)
    else {
        return false;
    };
    let Ok(quick_item) =
        MenuItem::with_id(app, MENU_QUICK_ACTIONS, "Quick Actions", true, None::<&str>)
    else {
        return false;
    };
    let Ok(dash_item) =
        MenuItem::with_id(app, MENU_DASHBOARD, "Open Dashboard", true, None::<&str>)
    else {
        return false;
    };
    let Ok(packs_item) = MenuItem::with_id(
        app,
        MENU_SOUNDPACKS,
        "Browse Soundpacks",
        true,
        None::<&str>,
    ) else {
        return false;
    };
    let Ok(sep) = PredefinedMenuItem::separator(app) else {
        return false;
    };
    let Ok(quit_item) = MenuItem::with_id(app, MENU_QUIT, "Quit Thocci", true, None::<&str>) else {
        return false;
    };

    let Ok(menu) = Menu::with_items(
        app,
        &[
            &toggle_item,
            &quick_item,
            &dash_item,
            &packs_item,
            &sep,
            &quit_item,
        ],
    ) else {
        return false;
    };

    let Ok(icon) = Image::from_bytes(TRAY_ICON_PNG) else {
        return false;
    };

    let state = app.state::<AppState>();
    {
        let mut slot = state
            .tray_toggle_item
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        *slot = Some(toggle_item);
    }

    let show_menu_on_left = cfg!(target_os = "linux");

    let builder = TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .tooltip("Thocci")
        .menu(&menu)
        .show_menu_on_left_click(show_menu_on_left)
        .on_menu_event(|app, event| handle_menu_event(app, event.id.as_ref()))
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                rect,
                ..
            } = event
            {
                let app = tray.app_handle();
                toggle_quick_actions_anchored(app, Some(rect));
            }
        });

    #[cfg(target_os = "macos")]
    let builder = builder.icon_as_template(true);

    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| builder.build(app))) {
        Ok(Ok(_)) => true,
        Ok(Err(err)) => {
            log::warn!(
                "System tray unavailable on this host ({err}); continuing with dashboard fallback"
            );
            false
        }
        Err(_) => {
            log::warn!(
                "System tray backend panicked during initialization; continuing with dashboard fallback"
            );
            false
        }
    }
}

pub fn refresh_tray_status(app: &AppHandle<Wry>, running: bool) {
    let state = app.state::<AppState>();
    let label = if running {
        "Disable Thocci"
    } else {
        "Enable Thocci"
    };
    if let Some(item) = state
        .tray_toggle_item
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
    {
        let _ = item.set_text(label);
    }
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let tooltip = if running {
            "Thocci — On"
        } else {
            "Thocci — Off"
        };
        let _ = tray.set_tooltip(Some(tooltip));
    }
}

fn handle_menu_event(app: &AppHandle<Wry>, id: &str) {
    match id {
        MENU_TOGGLE => {
            let app_handle = app.clone();
            tauri::async_runtime::spawn(async move {
                let _ = crate::toggle_daemon_internal(app_handle).await;
            });
        }
        MENU_QUICK_ACTIONS => {
            toggle_quick_actions_anchored(app, None);
        }
        MENU_DASHBOARD => {
            let _ = crate::show_dashboard_window(app, None);
        }
        MENU_SOUNDPACKS => {
            let _ = crate::show_dashboard_window(app, Some("soundpacks".to_owned()));
        }
        MENU_QUIT => {
            let app_handle = app.clone();
            tauri::async_runtime::spawn(async move {
                crate::shutdown_and_exit(app_handle).await;
            });
        }
        _ => {}
    }
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Lazily construct the Quick Actions WebView window on first demand so startup
/// and everyday dashboard use never spawn an idle second WebKit/WebView process.
/// Returns `(window, created_now)`.
fn ensure_quick_actions_window(app: &AppHandle<Wry>) -> Option<(tauri::WebviewWindow<Wry>, bool)> {
    if let Some(existing) = app.get_webview_window("quick-actions") {
        return Some((existing, false));
    }

    let builder = WebviewWindowBuilder::new(
        app,
        "quick-actions",
        WebviewUrl::App("index.html?view=quick-actions".into()),
    )
    .title("Thocci Quick Actions")
    .inner_size(QUICK_ACTIONS_LOGICAL_W, QUICK_ACTIONS_LOGICAL_H)
    .resizable(cfg!(target_os = "linux"))
    .decorations(false)
    .transparent(true)
    .always_on_top(true)
    .skip_taskbar(true)
    .shadow(false)
    .visible(false);

    let win = match builder.build() {
        Ok(win) => win,
        Err(err) => {
            log::error!("Failed to create Quick Actions window lazily: {err}");
            return None;
        }
    };

    #[cfg(not(target_os = "linux"))]
    {
        let _ = win.set_resizable(false);
    }

    Some((win, true))
}

pub fn toggle_quick_actions_anchored(app: &AppHandle<Wry>, anchor: Option<Rect>) {
    if let Some(win) = app.get_webview_window("quick-actions") {
        if win.is_visible().unwrap_or(false) {
            hide_quick_actions_window(app);
            return;
        }
    }

    // Avoid immediately re-opening if focus loss just hid the popover from the same tray click.
    let state = app.state::<AppState>();
    let last_blur = state.last_quick_actions_blur_ms.load(Ordering::Acquire);
    if now_ms().saturating_sub(last_blur) < 180 {
        return;
    }

    show_quick_actions_window(app, anchor);
}

pub fn show_quick_actions_window(app: &AppHandle<Wry>, anchor: Option<Rect>) {
    let Some((win, created_now)) = ensure_quick_actions_window(app) else {
        return;
    };
    #[cfg(not(target_os = "linux"))]
    let _ = created_now;

    app.state::<AppState>()
        .last_quick_actions_show_ms
        .store(now_ms(), Ordering::Release);

    #[cfg(target_os = "linux")]
    if crate::layer_shell::is_active() {
        let app_clone = app.clone();
        let win_clone = win.clone();
        let _ = app.run_on_main_thread(move || {
            if created_now {
                let (mon_w, mon_h) =
                    crate::layer_shell::current_monitor_logical_size().unwrap_or((1920, 1080));
                let _ = crate::layer_shell::setup_window(
                    &app_clone,
                    &win_clone,
                    "thocci-quick-actions",
                    crate::layer_shell::LayerKind::BarPopup,
                    crate::layer_shell::PanelRect {
                        x: 0,
                        y: 0,
                        width: mon_w,
                        height: mon_h,
                    },
                );
            }
            let placement = crate::layer_shell::prepare_bar_popup(
                "quick-actions",
                QUICK_ACTIONS_LOGICAL_W as i32,
                QUICK_ACTIONS_LOGICAL_H as i32,
            );
            *app_clone
                .state::<AppState>()
                .last_quick_actions_placement
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = Some(placement.clone());
            let _ = app_clone.emit_to(
                "quick-actions",
                "thocci://quick-actions-placement",
                &placement,
            );
            let _ = win_clone.show();
            let _ = win_clone.set_focus();
            let _ = app_clone.emit_to("quick-actions", "thocci://quick-actions-shown", ());
        });
        return;
    }

    position_quick_actions(&win, anchor);
    let _ = win.show();
    let _ = win.unminimize();
    let _ = win.set_focus();
    crate::platform::ensure_floating_overlay(
        "Thocci Quick Actions",
        QUICK_ACTIONS_LOGICAL_W as u32,
        QUICK_ACTIONS_LOGICAL_H as u32,
        false,
        true,
    );
    let _ = app.emit_to("quick-actions", "thocci://quick-actions-shown", ());
}

pub fn hide_quick_actions_window(app: &AppHandle<Wry>) {
    if app.get_webview_window("quick-actions").is_none() {
        return;
    }

    #[cfg(target_os = "linux")]
    if crate::layer_shell::is_active() {
        let app_clone = app.clone();
        let _ = app.run_on_main_thread(move || {
            crate::layer_shell::hide_window("quick-actions");
            let _ = app_clone.emit_to("quick-actions", "thocci://quick-actions-hidden", ());
        });
        return;
    }

    if let Some(win) = app.get_webview_window("quick-actions") {
        let _ = win.hide();
        let _ = app.emit_to("quick-actions", "thocci://quick-actions-hidden", ());
    }
}

fn position_quick_actions(win: &tauri::WebviewWindow<Wry>, anchor: Option<Rect>) {
    let monitors = win.available_monitors().unwrap_or_default();
    let default_monitor = win
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| win.primary_monitor().ok().flatten())
        .or_else(|| monitors.first().cloned());

    let (chosen_monitor, anchor_phys) = if let Some(rect) = anchor {
        let (ax, ay, aw, ah) = rect_to_physical(&rect, default_monitor.as_ref());
        let center_x = ax + aw / 2;
        let center_y = ay + ah / 2;
        let matched = monitors
            .iter()
            .find(|m| {
                let pos = m.position();
                let size = m.size();
                center_x >= pos.x
                    && center_x <= pos.x + size.width as i32
                    && center_y >= pos.y
                    && center_y <= pos.y + size.height as i32
            })
            .cloned()
            .or(default_monitor);
        (
            matched,
            Some(PixelRect {
                x: ax,
                y: ay,
                width: aw,
                height: ah,
            }),
        )
    } else {
        (default_monitor, None)
    };

    let Some(monitor) = chosen_monitor else {
        return;
    };

    let scale = monitor.scale_factor();
    let pop_w = (QUICK_ACTIONS_LOGICAL_W * scale).round() as i32;
    let pop_h = (QUICK_ACTIONS_LOGICAL_H * scale).round() as i32;
    let gap = (8.0 * scale).round() as i32;
    let margin = (12.0 * scale).round() as i32;

    let wa = monitor.work_area();
    let work_rect = PixelRect {
        x: wa.position.x,
        y: wa.position.y,
        width: wa.size.width as i32,
        height: wa.size.height as i32,
    };

    let (x, y) = match anchor_phys {
        Some(a) if a.width > 0 || a.height > 0 => {
            compute_popover_position(a, work_rect, (pop_w, pop_h), gap, margin)
        }
        _ => fallback_popover_position(work_rect, (pop_w, pop_h), margin),
    };

    let _ = win.set_size(PhysicalSize::new(pop_w.max(1) as u32, pop_h.max(1) as u32));
    let _ = win.set_position(PhysicalPosition::new(x, y));
}

fn rect_to_physical(rect: &Rect, monitor: Option<&tauri::Monitor>) -> (i32, i32, i32, i32) {
    let scale = monitor.map(|m| m.scale_factor()).unwrap_or(1.0);
    let pos = rect.position.to_physical::<i32>(scale);
    let size = rect.size.to_physical::<i32>(scale);
    (pos.x, pos.y, size.width, size.height)
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn missing_shared_libraries_return_false_without_panicking() {
        assert!(!can_dlopen_any(&[
            c"libthocci-nonexistent-tray-probe-1.so.0",
            c"libthocci-nonexistent-tray-probe-2.so",
        ]));
    }
}
