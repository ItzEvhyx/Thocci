//! Cross-platform menu-bar / system-tray host and Quick Actions popover routing.

use std::sync::atomic::Ordering;
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::image::Image;
#[cfg(not(target_os = "linux"))]
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
#[cfg(not(target_os = "linux"))]
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, Rect, WebviewUrl,
    WebviewWindowBuilder, Wry,
};

use crate::platform::{compute_popover_position, fallback_popover_position, PixelRect};
use crate::AppState;

#[cfg(not(target_os = "linux"))]
const TRAY_ID: &str = "thocci-tray";
#[cfg(not(target_os = "linux"))]
const MENU_TOGGLE: &str = "tray-toggle";
#[cfg(not(target_os = "linux"))]
const MENU_QUICK_ACTIONS: &str = "tray-quick-actions";
#[cfg(not(target_os = "linux"))]
const MENU_DASHBOARD: &str = "tray-dashboard";
#[cfg(not(target_os = "linux"))]
const MENU_SOUNDPACKS: &str = "tray-soundpacks";
#[cfg(not(target_os = "linux"))]
const MENU_QUIT: &str = "tray-quit";

const QUICK_ACTIONS_LOGICAL_W: f64 = 288.0;
const QUICK_ACTIONS_LOGICAL_H: f64 = 282.0;

#[cfg(target_os = "macos")]
const TRAY_ICON_PNG: &[u8] =
    include_bytes!("../../assets/desktop/brand/tray/thocci-tray-template-32.png");

#[cfg(target_os = "linux")]
const TRAY_ICON_22_PNG: &[u8] =
    include_bytes!("../../assets/desktop/brand/tray/thocci-tray-light-22.png");

#[cfg(not(target_os = "macos"))]
const TRAY_ICON_PNG: &[u8] =
    include_bytes!("../../assets/desktop/brand/tray/thocci-tray-light-32.png");

#[cfg(target_os = "linux")]
fn decode_png_to_ksni_icon(bytes: &[u8]) -> Option<ksni::Icon> {
    let img = Image::from_bytes(bytes).ok()?;
    let width = img.width() as i32;
    let height = img.height() as i32;
    let mut data = img.rgba().to_vec();
    for px in data.as_chunks_mut::<4>().0 {
        // StatusNotifierItem expects ARGB32 in network (big-endian) byte order.
        px.rotate_right(1);
    }
    Some(ksni::Icon {
        width,
        height,
        data,
    })
}

#[cfg(target_os = "linux")]
fn build_linux_tray_icons() -> Vec<ksni::Icon> {
    let mut icons = Vec::with_capacity(2);
    if let Some(icon_22) = decode_png_to_ksni_icon(TRAY_ICON_22_PNG) {
        icons.push(icon_22);
    }
    if let Some(icon_32) = decode_png_to_ksni_icon(TRAY_ICON_PNG) {
        icons.push(icon_32);
    }
    icons
}

/// Pure-Rust Linux StatusNotifierItem tray over D-Bus (`org.kde.StatusNotifierWatcher`).
///
/// Works natively on Quickshell, Waybar, Hyprpanel, AGS, KDE Plasma, GNOME
/// AppIndicator, Swaybar, Polybar, and X11 panels without requiring
/// `libayatana-appindicator3` or `libappindicator3` shared libraries.
#[cfg(target_os = "linux")]
pub(crate) struct LinuxTray {
    app: AppHandle<Wry>,
    running: bool,
    icons: Vec<ksni::Icon>,
}

#[cfg(target_os = "linux")]
impl ksni::Tray for LinuxTray {
    fn id(&self) -> String {
        "thocci".to_owned()
    }

    fn category(&self) -> ksni::Category {
        ksni::Category::ApplicationStatus
    }

    fn title(&self) -> String {
        "Thocci".to_owned()
    }

    fn status(&self) -> ksni::Status {
        ksni::Status::Active
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        self.icons.clone()
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            icon_name: String::new(),
            icon_pixmap: Vec::new(),
            title: if self.running {
                "Thocci — On".to_owned()
            } else {
                "Thocci — Off".to_owned()
            },
            description: "Click to open Quick Actions".to_owned(),
        }
    }

    fn activate(&mut self, x: i32, y: i32) {
        let anchor = if x > 0 || y > 0 {
            Some(Rect {
                position: tauri::Position::Physical(PhysicalPosition::new(x, y)),
                size: tauri::Size::Physical(PhysicalSize::new(24, 24)),
            })
        } else {
            None
        };
        toggle_quick_actions_anchored(&self.app, anchor);
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::{MenuItem as KsniMenuItem, StandardItem};

        let toggle_label = if self.running {
            "Disable Thocci"
        } else {
            "Enable Thocci"
        };

        vec![
            StandardItem {
                label: toggle_label.to_owned(),
                activate: Box::new(|this: &mut Self| {
                    let app_handle = this.app.clone();
                    tauri::async_runtime::spawn(async move {
                        let _ = crate::toggle_daemon_internal(app_handle).await;
                    });
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Quick Actions".to_owned(),
                activate: Box::new(|this: &mut Self| {
                    toggle_quick_actions_anchored(&this.app, None);
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Open Dashboard".to_owned(),
                activate: Box::new(|this: &mut Self| {
                    let _ = crate::show_dashboard_window(&this.app, None);
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Browse Soundpacks".to_owned(),
                activate: Box::new(|this: &mut Self| {
                    let _ = crate::show_dashboard_window(&this.app, Some("soundpacks".to_owned()));
                }),
                ..Default::default()
            }
            .into(),
            KsniMenuItem::Separator,
            StandardItem {
                label: "Quit Thocci".to_owned(),
                activate: Box::new(|this: &mut Self| {
                    let app_handle = this.app.clone();
                    tauri::async_runtime::spawn(async move {
                        crate::shutdown_and_exit(app_handle).await;
                    });
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

#[cfg(target_os = "linux")]
pub async fn setup_tray_async(app: &AppHandle<Wry>, initial_running: bool) -> bool {
    use ksni::TrayMethods;

    let tray = LinuxTray {
        app: app.clone(),
        running: initial_running,
        icons: build_linux_tray_icons(),
    };

    match tray.spawn().await {
        Ok(handle) => {
            let state = app.state::<AppState>();
            *state
                .linux_tray_handle
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = Some(handle);
            true
        }
        Err(err) => {
            log::warn!(
                "Linux StatusNotifierItem tray unavailable on this host ({err}); continuing with dashboard fallback"
            );
            false
        }
    }
}

#[cfg(target_os = "linux")]
pub fn setup_tray(app: &AppHandle<Wry>, initial_running: bool) -> bool {
    let (tx, rx) = std::sync::mpsc::channel();
    let app_clone = app.clone();
    tauri::async_runtime::spawn(async move {
        let ok = setup_tray_async(&app_clone, initial_running).await;
        let _ = tx.send(ok);
    });
    rx.recv_timeout(std::time::Duration::from_millis(1500))
        .unwrap_or(false)
}

#[cfg(not(target_os = "linux"))]
pub async fn setup_tray_async(app: &AppHandle<Wry>, initial_running: bool) -> bool {
    setup_tray(app, initial_running)
}

#[cfg(not(target_os = "linux"))]
pub fn setup_tray(app: &AppHandle<Wry>, initial_running: bool) -> bool {
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

    let builder = TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .tooltip("Thocci")
        .menu(&menu)
        .show_menu_on_left_click(false)
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
    #[cfg(target_os = "linux")]
    {
        let handle = app
            .state::<AppState>()
            .linux_tray_handle
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if let Some(handle) = handle {
            tauri::async_runtime::spawn(async move {
                let _ = handle.update(|tray| tray.running = running).await;
            });
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
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
}

pub async fn shutdown_tray(app: &AppHandle<Wry>) {
    #[cfg(target_os = "linux")]
    {
        let handle = app
            .state::<AppState>()
            .linux_tray_handle
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some(handle) = handle {
            handle.shutdown().await;
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = app;
    }
}

#[cfg(not(target_os = "linux"))]
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
    fn linux_tray_icons_decode_to_valid_argb32_pixmaps() {
        let icons = build_linux_tray_icons();
        assert_eq!(icons.len(), 2);
        assert_eq!((icons[0].width, icons[0].height), (22, 22));
        assert_eq!(icons[0].data.len(), 22 * 22 * 4);
        assert_eq!((icons[1].width, icons[1].height), (32, 32));
        assert_eq!(icons[1].data.len(), 32 * 32 * 4);
    }
}
