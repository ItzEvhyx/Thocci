//! Cross-platform menu-bar / system-tray host, OS-matched tray contrast, and
//! Quick Actions popover sizing/routing.
//!
//! Tray appearance strategy:
//! - **macOS**: uses the native template asset (`thocci-tray-template-32.png`) with
//!   `icon_as_template(true)` so AppKit adapts the mark to the menu bar surface.
//! - **Windows**: reads `HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize`
//!   preferring `SystemUsesLightTheme` (the taskbar surface) over `AppsUseLightTheme`,
//!   uses the black mark (`thocci-tray-dark-32.png`) on a light taskbar and the white
//!   mark (`thocci-tray-light-32.png`) on a dark taskbar, and watches that registry key
//!   for live updates via `RegNotifyChangeKeyValue`. Fallback when unreadable: dark
//!   surface (white mark).
//! - **Linux**: probes host appearance in priority order:
//!   1. XDG Desktop Portal `org.freedesktop.portal.Settings` (`org.freedesktop.appearance` / `color-scheme`)
//!   2. GNOME / GTK `gsettings` (`org.gnome.desktop.interface` `color-scheme`, then `gtk-theme`)
//!   3. KDE Plasma `kdeglobals` (`[General]` `ColorScheme`)
//!   4. `GTK_THEME` environment variable and `~/.config/gtk-3.0/settings.ini`
//!   5. Documented fallback when no host appearance data is reported: dark panel surface
//!      (white mark `thocci-tray-light-{22,32}.png`), matching standard Linux panels.
//!
//!   Updates the existing `ksni` StatusNotifierItem pixmaps (preserving both 22 px and
//!   32 px ARGB32 variants) live on `org.freedesktop.portal.Settings::SettingChanged`.
//!
//! The dashboard's explicit Light/Dark toggle is persisted in `ui-prefs.json` and kept
//! strictly independent from the host tray mark.

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

use crate::platform::{compute_popover_geometry, PixelRect};
use crate::{AppState, QuickActionsPlacement};

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

pub const QUICK_ACTIONS_LOGICAL_W: f64 = 288.0;
pub const QUICK_ACTIONS_COLLAPSED_H: f64 = 320.0;
pub const QUICK_ACTIONS_MAX_H: f64 = 600.0;

#[cfg(target_os = "macos")]
const TRAY_ICON_TEMPLATE_32_PNG: &[u8] =
    include_bytes!("../../assets/desktop/brand/tray/thocci-tray-template-32.png");

/// White mark PNG exports — used on a dark tray/taskbar surface.
#[cfg(target_os = "linux")]
const TRAY_MARK_WHITE_22_PNG: &[u8] =
    include_bytes!("../../assets/desktop/brand/tray/thocci-tray-light-22.png");
#[cfg(not(target_os = "macos"))]
const TRAY_MARK_WHITE_32_PNG: &[u8] =
    include_bytes!("../../assets/desktop/brand/tray/thocci-tray-light-32.png");

/// Black mark PNG exports — used on a light tray/taskbar surface.
#[cfg(target_os = "linux")]
const TRAY_MARK_BLACK_22_PNG: &[u8] =
    include_bytes!("../../assets/desktop/brand/tray/thocci-tray-dark-22.png");
#[cfg(not(target_os = "macos"))]
const TRAY_MARK_BLACK_32_PNG: &[u8] =
    include_bytes!("../../assets/desktop/brand/tray/thocci-tray-dark-32.png");

/// Resolved host panel/taskbar surface brightness used to choose contrasting
/// monochrome tray artwork.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraySurfaceAppearance {
    /// Light tray/taskbar surface -> render the black Thocci mark (`thocci-tray-dark-*`).
    Light,
    /// Dark tray/taskbar surface -> render the white Thocci mark (`thocci-tray-light-*`).
    Dark,
}

#[cfg(not(target_os = "macos"))]
pub fn tray_mark_png_32(surface: TraySurfaceAppearance) -> &'static [u8] {
    match surface {
        TraySurfaceAppearance::Light => TRAY_MARK_BLACK_32_PNG,
        TraySurfaceAppearance::Dark => TRAY_MARK_WHITE_32_PNG,
    }
}

#[cfg(target_os = "linux")]
pub fn tray_mark_png_22(surface: TraySurfaceAppearance) -> &'static [u8] {
    match surface {
        TraySurfaceAppearance::Light => TRAY_MARK_BLACK_22_PNG,
        TraySurfaceAppearance::Dark => TRAY_MARK_WHITE_22_PNG,
    }
}

/// Parse XDG Desktop Portal `org.freedesktop.appearance` `color-scheme`:
/// `1` = prefer dark, `2` = prefer light, `0` = no preference.
#[cfg(any(target_os = "linux", test))]
pub fn parse_portal_color_scheme(code: u32) -> Option<TraySurfaceAppearance> {
    match code {
        1 => Some(TraySurfaceAppearance::Dark),
        2 => Some(TraySurfaceAppearance::Light),
        _ => None,
    }
}

/// Parse GNOME `gsettings get org.gnome.desktop.interface color-scheme`.
#[cfg(any(target_os = "linux", test))]
pub fn parse_gsettings_color_scheme(raw: &str) -> Option<TraySurfaceAppearance> {
    let cleaned = raw.trim().trim_matches('\'').trim_matches('"');
    match cleaned {
        "prefer-dark" => Some(TraySurfaceAppearance::Dark),
        "prefer-light" => Some(TraySurfaceAppearance::Light),
        _ => None,
    }
}

/// Infer surface appearance from a desktop theme name (e.g. `Adwaita-dark`, `BreezeLight`).
#[cfg(any(target_os = "linux", test))]
pub fn parse_theme_name_appearance(raw: &str) -> Option<TraySurfaceAppearance> {
    let cleaned = raw.trim().trim_matches('\'').trim_matches('"');
    if cleaned.is_empty() {
        return None;
    }
    let lower = cleaned.to_ascii_lowercase();
    if lower.contains("dark") || lower.contains("noir") || lower.contains("night") {
        Some(TraySurfaceAppearance::Dark)
    } else {
        Some(TraySurfaceAppearance::Light)
    }
}

/// Parse KDE Plasma `kdeglobals` `[General]` `ColorScheme=...`.
#[cfg(any(target_os = "linux", test))]
pub fn parse_kdeglobals_appearance(contents: &str) -> Option<TraySurfaceAppearance> {
    let mut in_general = false;
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_general = trimmed.eq_ignore_ascii_case("[General]");
            continue;
        }
        if in_general {
            if let Some(val) = trimmed.strip_prefix("ColorScheme=") {
                return parse_theme_name_appearance(val);
            }
        }
    }
    None
}

/// Parse GTK 3/4 `settings.ini` appearance preference.
#[cfg(any(target_os = "linux", test))]
pub fn parse_gtk_settings_ini_appearance(contents: &str) -> Option<TraySurfaceAppearance> {
    let mut theme_name: Option<String> = None;
    for line in contents.lines() {
        let trimmed = line.trim();
        if let Some((key, val)) = trimmed.split_once('=') {
            let k = key.trim();
            let v = val.trim();
            if k.eq_ignore_ascii_case("gtk-application-prefer-dark-theme") {
                match v.to_ascii_lowercase().as_str() {
                    "1" | "true" | "yes" => return Some(TraySurfaceAppearance::Dark),
                    "0" | "false" | "no" => return Some(TraySurfaceAppearance::Light),
                    _ => {}
                }
            } else if k.eq_ignore_ascii_case("gtk-theme-name") {
                theme_name = Some(v.to_owned());
            }
        }
    }
    theme_name.as_deref().and_then(parse_theme_name_appearance)
}

/// Resolve Windows taskbar appearance, preferring `SystemUsesLightTheme` (taskbar
/// surface) over `AppsUseLightTheme`, defaulting to Dark if neither is present.
#[cfg(any(windows, test))]
pub fn parse_windows_personalize_values(
    system_uses_light: Option<u32>,
    apps_use_light: Option<u32>,
) -> TraySurfaceAppearance {
    match system_uses_light.or(apps_use_light) {
        Some(1) => TraySurfaceAppearance::Light,
        Some(0) => TraySurfaceAppearance::Dark,
        _ => TraySurfaceAppearance::Dark,
    }
}

#[cfg(target_os = "linux")]
fn extract_u32_from_zvariant(value: &zbus::zvariant::Value<'_>) -> Option<u32> {
    match value {
        zbus::zvariant::Value::U32(n) => Some(*n),
        zbus::zvariant::Value::Value(inner) => extract_u32_from_zvariant(inner),
        _ => None,
    }
}

#[cfg(target_os = "linux")]
async fn query_portal_color_scheme(conn: &zbus::Connection) -> Option<TraySurfaceAppearance> {
    let proxy = zbus::Proxy::new(
        conn,
        "org.freedesktop.portal.Desktop",
        "/org/freedesktop/portal/desktop",
        "org.freedesktop.portal.Settings",
    )
    .await
    .ok()?;

    if let Ok(val) = proxy
        .call::<_, _, zbus::zvariant::OwnedValue>(
            "ReadOne",
            &("org.freedesktop.appearance", "color-scheme"),
        )
        .await
    {
        if let Some(code) = extract_u32_from_zvariant(&val) {
            if let Some(app) = parse_portal_color_scheme(code) {
                return Some(app);
            }
        }
    }

    if let Ok(val) = proxy
        .call::<_, _, zbus::zvariant::OwnedValue>(
            "Read",
            &("org.freedesktop.appearance", "color-scheme"),
        )
        .await
    {
        if let Some(code) = extract_u32_from_zvariant(&val) {
            if let Some(app) = parse_portal_color_scheme(code) {
                return Some(app);
            }
        }
    }

    None
}

#[cfg(target_os = "linux")]
fn detect_linux_fallback_appearance() -> TraySurfaceAppearance {
    use std::process::Command;

    if let Ok(out) = Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "color-scheme"])
        .output()
    {
        if out.status.success() {
            if let Some(found) = parse_gsettings_color_scheme(&String::from_utf8_lossy(&out.stdout))
            {
                return found;
            }
        }
    }

    if let Ok(out) = Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "gtk-theme"])
        .output()
    {
        if out.status.success() {
            if let Some(found) = parse_theme_name_appearance(&String::from_utf8_lossy(&out.stdout))
            {
                return found;
            }
        }
    }

    let config_root = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")));

    if let Some(cfg_dir) = &config_root {
        if let Ok(kde) = std::fs::read_to_string(cfg_dir.join("kdeglobals")) {
            if let Some(found) = parse_kdeglobals_appearance(&kde) {
                return found;
            }
        }
    }

    if let Ok(gtk_theme) = std::env::var("GTK_THEME") {
        if let Some(found) = parse_theme_name_appearance(&gtk_theme) {
            return found;
        }
    }

    if let Some(cfg_dir) = &config_root {
        for sub in ["gtk-4.0/settings.ini", "gtk-3.0/settings.ini"] {
            if let Ok(ini) = std::fs::read_to_string(cfg_dir.join(sub)) {
                if let Some(found) = parse_gtk_settings_ini_appearance(&ini) {
                    return found;
                }
            }
        }
    }

    // Documented fallback when no host appearance signal is available: assume dark panel
    // surface (standard on Waybar, Hyprpanel, Quickshell, GNOME Shell, Polybar) and use
    // the white Thocci mark.
    TraySurfaceAppearance::Dark
}

#[cfg(target_os = "linux")]
async fn detect_linux_tray_appearance(conn: Option<&zbus::Connection>) -> TraySurfaceAppearance {
    if let Some(c) = conn {
        if let Some(app) = query_portal_color_scheme(c).await {
            return app;
        }
    } else if let Ok(c) = zbus::Connection::session().await {
        if let Some(app) = query_portal_color_scheme(&c).await {
            return app;
        }
    }
    detect_linux_fallback_appearance()
}

#[cfg(windows)]
fn read_windows_registry_dword(subkey: &[u16], value_name: &[u16]) -> Option<u32> {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ, REG_DWORD,
    };

    unsafe {
        let mut hkey: HKEY = std::ptr::null_mut();
        if RegOpenKeyExW(HKEY_CURRENT_USER, subkey.as_ptr(), 0, KEY_READ, &mut hkey)
            != ERROR_SUCCESS
        {
            return None;
        }
        let mut data_type: u32 = 0;
        let mut value: u32 = 0;
        let mut size: u32 = std::mem::size_of::<u32>() as u32;
        let status = RegQueryValueExW(
            hkey,
            value_name.as_ptr(),
            std::ptr::null_mut(),
            &mut data_type,
            (&mut value as *mut u32).cast::<u8>(),
            &mut size,
        );
        RegCloseKey(hkey);
        if status == ERROR_SUCCESS && data_type == REG_DWORD {
            Some(value)
        } else {
            None
        }
    }
}

#[cfg(windows)]
fn wide_null(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
fn detect_windows_tray_appearance() -> TraySurfaceAppearance {
    let subkey = wide_null(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
    let sys_val = wide_null("SystemUsesLightTheme");
    let app_val = wide_null("AppsUseLightTheme");
    let system_uses_light = read_windows_registry_dword(&subkey, &sys_val);
    let apps_use_light = read_windows_registry_dword(&subkey, &app_val);
    parse_windows_personalize_values(system_uses_light, apps_use_light)
}

#[cfg(windows)]
pub fn refresh_windows_tray_appearance(app: &AppHandle<Wry>) {
    let surface = detect_windows_tray_appearance();
    let Ok(icon) = Image::from_bytes(tray_mark_png_32(surface)) else {
        return;
    };
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_icon(Some(icon));
    }
}

#[cfg(windows)]
fn spawn_windows_appearance_watcher(app: AppHandle<Wry>) {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegNotifyChangeKeyValue, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, KEY_NOTIFY,
        KEY_READ, REG_NOTIFY_CHANGE_LAST_SET,
    };

    let _ = std::thread::Builder::new()
        .name("thocci-tray-theme".to_owned())
        .spawn(move || {
            let subkey = wide_null(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
            let mut hkey: HKEY = std::ptr::null_mut();
            let opened = unsafe {
                RegOpenKeyExW(
                    HKEY_CURRENT_USER,
                    subkey.as_ptr(),
                    0,
                    KEY_READ | KEY_NOTIFY,
                    &mut hkey,
                )
            };
            if opened != ERROR_SUCCESS {
                return;
            }
            let mut last = detect_windows_tray_appearance();
            loop {
                if app.state::<AppState>().is_quitting.load(Ordering::Acquire) {
                    break;
                }
                let rc = unsafe {
                    RegNotifyChangeKeyValue(
                        hkey,
                        0,
                        REG_NOTIFY_CHANGE_LAST_SET,
                        std::ptr::null_mut(),
                        0,
                    )
                };
                if rc != ERROR_SUCCESS
                    || app.state::<AppState>().is_quitting.load(Ordering::Acquire)
                {
                    break;
                }
                let next = detect_windows_tray_appearance();
                if next != last {
                    last = next;
                    refresh_windows_tray_appearance(&app);
                }
            }
            unsafe {
                RegCloseKey(hkey);
            }
        });
}

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
pub fn build_linux_tray_icons(surface: TraySurfaceAppearance) -> Vec<ksni::Icon> {
    let mut icons = Vec::with_capacity(2);
    if let Some(icon_22) = decode_png_to_ksni_icon(tray_mark_png_22(surface)) {
        icons.push(icon_22);
    }
    if let Some(icon_32) = decode_png_to_ksni_icon(tray_mark_png_32(surface)) {
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
                label: "Open Home".to_owned(),
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
fn spawn_linux_appearance_watcher(app: AppHandle<Wry>, initial: TraySurfaceAppearance) {
    use futures_util::StreamExt;

    tauri::async_runtime::spawn(async move {
        let Ok(conn) = zbus::Connection::session().await else {
            return;
        };
        let Ok(proxy) = zbus::Proxy::new(
            &conn,
            "org.freedesktop.portal.Desktop",
            "/org/freedesktop/portal/desktop",
            "org.freedesktop.portal.Settings",
        )
        .await
        else {
            return;
        };
        let Ok(mut stream) = proxy.receive_signal("SettingChanged").await else {
            return;
        };

        let mut last = initial;
        while let Some(msg) = stream.next().await {
            if app.state::<AppState>().is_quitting.load(Ordering::Acquire) {
                break;
            }
            let Ok((ns, key, value)) = msg
                .body()
                .deserialize::<(String, String, zbus::zvariant::OwnedValue)>()
            else {
                continue;
            };
            let relevant = (ns == "org.freedesktop.appearance" && key == "color-scheme")
                || (ns == "org.gnome.desktop.interface"
                    && (key == "color-scheme" || key == "gtk-theme"));
            if !relevant {
                continue;
            }

            let next = if ns == "org.freedesktop.appearance" && key == "color-scheme" {
                extract_u32_from_zvariant(&value)
                    .and_then(parse_portal_color_scheme)
                    .unwrap_or_else(detect_linux_fallback_appearance)
            } else {
                detect_linux_tray_appearance(Some(&conn)).await
            };

            if next == last {
                continue;
            }
            last = next;

            let handle = app
                .state::<AppState>()
                .linux_tray_handle
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            let Some(handle) = handle else {
                break;
            };
            let next_icons = build_linux_tray_icons(next);
            let _ = handle.update(|tray| tray.icons = next_icons).await;
        }
    });
}

#[cfg(target_os = "linux")]
pub async fn setup_tray_async(app: &AppHandle<Wry>, initial_running: bool) -> bool {
    use ksni::TrayMethods;

    let initial_surface = detect_linux_tray_appearance(None).await;
    let tray = LinuxTray {
        app: app.clone(),
        running: initial_running,
        icons: build_linux_tray_icons(initial_surface),
    };

    match tray.spawn().await {
        Ok(handle) => {
            let state = app.state::<AppState>();
            *state
                .linux_tray_handle
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = Some(handle);
            spawn_linux_appearance_watcher(app.clone(), initial_surface);
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
    let Ok(dash_item) = MenuItem::with_id(app, MENU_DASHBOARD, "Open Home", true, None::<&str>)
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

    #[cfg(target_os = "macos")]
    let icon_bytes = TRAY_ICON_TEMPLATE_32_PNG;
    #[cfg(windows)]
    let icon_bytes = tray_mark_png_32(detect_windows_tray_appearance());
    #[cfg(all(not(target_os = "macos"), not(windows)))]
    let icon_bytes = tray_mark_png_32(TraySurfaceAppearance::Dark);

    let Ok(icon) = Image::from_bytes(icon_bytes) else {
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
        Ok(Ok(_)) => {
            #[cfg(windows)]
            spawn_windows_appearance_watcher(app.clone());
            true
        }
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

/// Clamp and validate a requested Quick Actions popover logical height.
pub fn validate_quick_actions_height(requested: f64) -> Result<f64, String> {
    if !requested.is_finite() {
        return Err("Quick Actions height must be a finite number.".to_owned());
    }
    Ok(requested.clamp(QUICK_ACTIONS_COLLAPSED_H, QUICK_ACTIONS_MAX_H))
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
    .inner_size(QUICK_ACTIONS_LOGICAL_W, QUICK_ACTIONS_COLLAPSED_H)
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

    let state = app.state::<AppState>();
    state
        .last_quick_actions_show_ms
        .store(now_ms(), Ordering::Release);
    *state
        .last_quick_actions_anchor
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = anchor;
    let current_height = *state
        .quick_actions_height
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let desired_h = current_height.clamp(QUICK_ACTIONS_COLLAPSED_H, QUICK_ACTIONS_MAX_H);

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
                desired_h.round() as i32,
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

    let applied_h = match position_quick_actions(&win, anchor, desired_h, true) {
        Some((placement, clamped_logical_h)) => {
            *state
                .last_quick_actions_placement
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = Some(placement.clone());
            let _ = app.emit_to(
                "quick-actions",
                "thocci://quick-actions-placement",
                &placement,
            );
            clamped_logical_h
        }
        None => desired_h,
    };

    let _ = win.show();
    let _ = win.unminimize();
    let _ = win.set_focus();
    crate::platform::ensure_floating_overlay(
        "Thocci Quick Actions",
        QUICK_ACTIONS_LOGICAL_W as u32,
        applied_h.round() as u32,
        false,
        true,
    );
    let _ = app.emit_to("quick-actions", "thocci://quick-actions-shown", ());
}

/// Resize and reposition the Quick Actions popover cleanly on its actual tray
/// monitor, validating and clamping height to the monitor's usable work area.
pub fn resize_quick_actions_window(app: &AppHandle<Wry>, requested_h: f64) -> Result<f64, String> {
    let clamped = validate_quick_actions_height(requested_h)?;
    let state = app.state::<AppState>();
    *state
        .quick_actions_height
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = clamped;

    #[cfg(target_os = "linux")]
    if crate::layer_shell::is_active() {
        let app_clone = app.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        app.run_on_main_thread(move || {
            let res = crate::layer_shell::resize_bar_popup(
                "quick-actions",
                QUICK_ACTIONS_LOGICAL_W as i32,
                clamped.round() as i32,
            );
            if let Some((placement, actual_h)) = res {
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
                let _ = tx.send(f64::from(actual_h));
            } else {
                let _ = tx.send(clamped);
            }
        })
        .map_err(|err| err.to_string())?;
        return Ok(rx
            .recv_timeout(std::time::Duration::from_millis(500))
            .unwrap_or(clamped));
    }

    let Some(win) = app.get_webview_window("quick-actions") else {
        return Ok(clamped);
    };
    let anchor = *state
        .last_quick_actions_anchor
        .lock()
        .unwrap_or_else(|e| e.into_inner());

    let applied_h = match position_quick_actions(&win, anchor, clamped, false) {
        Some((placement, actual_h)) => {
            *state
                .last_quick_actions_placement
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = Some(placement.clone());
            let _ = app.emit_to(
                "quick-actions",
                "thocci://quick-actions-placement",
                &placement,
            );
            actual_h
        }
        None => clamped,
    };

    crate::platform::ensure_floating_overlay(
        "Thocci Quick Actions",
        QUICK_ACTIONS_LOGICAL_W as u32,
        applied_h.round() as u32,
        false,
        false,
    );
    Ok(applied_h)
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

fn position_quick_actions(
    win: &tauri::WebviewWindow<Wry>,
    anchor: Option<Rect>,
    desired_logical_h: f64,
    animate: bool,
) -> Option<(QuickActionsPlacement, f64)> {
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

    let monitor = chosen_monitor?;

    let scale = monitor.scale_factor().max(0.5);
    let desired_w = (QUICK_ACTIONS_LOGICAL_W * scale).round() as i32;
    let desired_h = (desired_logical_h * scale).round() as i32;
    let gap = (8.0 * scale).round() as i32;
    let margin = (12.0 * scale).round() as i32;

    let wa = monitor.work_area();
    let work_rect = PixelRect {
        x: wa.position.x,
        y: wa.position.y,
        width: wa.size.width as i32,
        height: wa.size.height as i32,
    };

    let (x, y, pop_w, pop_h, max_phys_h) =
        compute_popover_geometry(anchor_phys, work_rect, (desired_w, desired_h), gap, margin);

    let _ = win.set_size(PhysicalSize::new(pop_w.max(1) as u32, pop_h.max(1) as u32));
    let _ = win.set_position(PhysicalPosition::new(x, y));

    let logical_max_h = ((f64::from(max_phys_h) / scale).floor() as i32)
        .clamp(QUICK_ACTIONS_COLLAPSED_H as i32, QUICK_ACTIONS_MAX_H as i32);
    let actual_logical_h = (f64::from(pop_h) / scale)
        .round()
        .clamp(QUICK_ACTIONS_COLLAPSED_H, f64::from(logical_max_h));

    let placement = QuickActionsPlacement {
        layer_shell: false,
        x: (f64::from(x) / scale).round() as i32,
        y: (f64::from(y) / scale).round() as i32,
        screen_width: (f64::from(work_rect.width) / scale).round() as i32,
        screen_height: (f64::from(work_rect.height) / scale).round() as i32,
        max_height: logical_max_h,
        animate,
    };
    Some((placement, actual_logical_h))
}

fn rect_to_physical(rect: &Rect, monitor: Option<&tauri::Monitor>) -> (i32, i32, i32, i32) {
    let scale = monitor.map(|m| m.scale_factor()).unwrap_or(1.0);
    let pos = rect.position.to_physical::<i32>(scale);
    let size = rect.size.to_physical::<i32>(scale);
    (pos.x, pos.y, size.width, size.height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_and_clamps_quick_actions_height() {
        assert!(validate_quick_actions_height(f64::NAN).is_err());
        assert!(validate_quick_actions_height(f64::INFINITY).is_err());
        assert_eq!(validate_quick_actions_height(100.0).unwrap(), 320.0);
        assert_eq!(validate_quick_actions_height(444.0).unwrap(), 444.0);
        assert_eq!(validate_quick_actions_height(9999.0).unwrap(), 600.0);
    }

    #[test]
    fn windows_personalize_prefers_taskbar_surface_over_app_theme() {
        // Light taskbar + dark apps -> black mark (Light surface)
        assert_eq!(
            parse_windows_personalize_values(Some(1), Some(0)),
            TraySurfaceAppearance::Light
        );
        // Dark taskbar + light apps -> white mark (Dark surface)
        assert_eq!(
            parse_windows_personalize_values(Some(0), Some(1)),
            TraySurfaceAppearance::Dark
        );
        // Fallback to AppsUseLightTheme if SystemUsesLightTheme missing
        assert_eq!(
            parse_windows_personalize_values(None, Some(1)),
            TraySurfaceAppearance::Light
        );
        // Fallback when unreadable -> Dark surface
        assert_eq!(
            parse_windows_personalize_values(None, None),
            TraySurfaceAppearance::Dark
        );
    }

    #[test]
    fn linux_appearance_parsers_handle_portal_gsettings_kde_and_gtk() {
        assert_eq!(
            parse_portal_color_scheme(1),
            Some(TraySurfaceAppearance::Dark)
        );
        assert_eq!(
            parse_portal_color_scheme(2),
            Some(TraySurfaceAppearance::Light)
        );
        assert_eq!(parse_portal_color_scheme(0), None);

        assert_eq!(
            parse_gsettings_color_scheme("'prefer-dark'\n"),
            Some(TraySurfaceAppearance::Dark)
        );
        assert_eq!(
            parse_gsettings_color_scheme("'prefer-light'\n"),
            Some(TraySurfaceAppearance::Light)
        );
        assert_eq!(parse_gsettings_color_scheme("'default'\n"), None);

        assert_eq!(
            parse_kdeglobals_appearance("[General]\nColorScheme=BreezeLight\n"),
            Some(TraySurfaceAppearance::Light)
        );
        assert_eq!(
            parse_kdeglobals_appearance("[General]\nColorScheme=BreezeDark\n"),
            Some(TraySurfaceAppearance::Dark)
        );

        assert_eq!(
            parse_gtk_settings_ini_appearance("[Settings]\ngtk-application-prefer-dark-theme=0\n"),
            Some(TraySurfaceAppearance::Light)
        );
        assert_eq!(
            parse_gtk_settings_ini_appearance("[Settings]\ngtk-theme-name=Adwaita-dark\n"),
            Some(TraySurfaceAppearance::Dark)
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_tray_icons_decode_both_appearances_and_sizes_with_contrasting_marks() {
        let light_surface = build_linux_tray_icons(TraySurfaceAppearance::Light);
        let dark_surface = build_linux_tray_icons(TraySurfaceAppearance::Dark);
        assert_eq!(light_surface.len(), 2);
        assert_eq!(dark_surface.len(), 2);
        assert_eq!((light_surface[0].width, light_surface[0].height), (22, 22));
        assert_eq!(light_surface[0].data.len(), 22 * 22 * 4);
        assert_eq!((light_surface[1].width, light_surface[1].height), (32, 32));
        assert_eq!(light_surface[1].data.len(), 32 * 32 * 4);

        // Verify average RGB of opaque pixels: light surface uses black mark (< 60),
        // dark surface uses white mark (> 200).
        let avg_luma = |icon: &ksni::Icon| -> f64 {
            let mut sum = 0u64;
            let mut count = 0u64;
            for px in icon.data.as_chunks::<4>().0 {
                // ARGB32 big-endian
                if px[0] > 200 {
                    sum += u64::from(px[1]) + u64::from(px[2]) + u64::from(px[3]);
                    count += 3;
                }
            }
            assert!(count > 0, "expected opaque tray pixels");
            sum as f64 / count as f64
        };

        assert!(
            avg_luma(&light_surface[1]) < 60.0,
            "light tray surface must use black mark"
        );
        assert!(
            avg_luma(&dark_surface[1]) > 200.0,
            "dark tray surface must use white mark"
        );
    }
}
