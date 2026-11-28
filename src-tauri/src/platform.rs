//! Platform diagnostics, permission guidance, and popover placement math.

use std::path::Path;

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlatformInfo {
    pub app_version: String,
    pub os: String,
    pub arch: String,
    pub session_type: Option<String>,
    pub tray_available: bool,
    pub supports_tray_click_popover: bool,
    pub layer_shell_active: bool,
    pub input_permission_ready: Option<bool>,
    pub daemon_exe: String,
    pub permission_title: String,
    pub permission_steps: Vec<String>,
}

/// Compute a clamped physical-pixel top-left position for the Quick Actions popover.
///
/// - Anchors horizontally centered on `anchor`.
/// - Places below `anchor` when `anchor` sits in the upper half of `work_area`
///   (e.g. macOS menu bar or top Linux panel) and above `anchor` when `anchor`
///   sits in the lower half (e.g. Windows bottom taskbar).
/// - Clamps strictly inside `work_area` with an inset margin on all displays and
///   DPI scales.
pub fn compute_popover_position(
    anchor: PixelRect,
    work_area: PixelRect,
    popover_size: (i32, i32),
    gap_px: i32,
    margin_px: i32,
) -> (i32, i32) {
    let (pop_w, pop_h) = popover_size;
    let anchor_center_x = anchor.x + anchor.width / 2;
    let anchor_center_y = anchor.y + anchor.height / 2;
    let work_mid_y = work_area.y + work_area.height / 2;

    let raw_x = anchor_center_x - pop_w / 2;
    let raw_y = if anchor_center_y <= work_mid_y {
        anchor.y + anchor.height + gap_px
    } else {
        anchor.y - pop_h - gap_px
    };

    let min_x = work_area.x + margin_px;
    let max_x = (work_area.x + work_area.width - pop_w - margin_px).max(min_x);
    let min_y = work_area.y + margin_px;
    let max_y = (work_area.y + work_area.height - pop_h - margin_px).max(min_y);

    (raw_x.clamp(min_x, max_x), raw_y.clamp(min_y, max_y))
}

/// Compute clamped physical-pixel geometry `(x, y, width, height, max_height)`
/// for a Quick Actions popover on its actual tray monitor.
///
/// Handles top panels (macOS menu bar / Linux top bar), bottom panels (Windows
/// taskbar / bottom Linux panel), short work areas, and mixed-DPI scaling.
pub fn compute_popover_geometry(
    anchor: Option<PixelRect>,
    work_area: PixelRect,
    desired_size: (i32, i32),
    gap_px: i32,
    margin_px: i32,
) -> (i32, i32, i32, i32, i32) {
    let max_w = (work_area.width - 2 * margin_px).max(1);
    let work_max_h = (work_area.height - 2 * margin_px).max(1);

    let max_h = match anchor {
        Some(a) if a.width > 0 || a.height > 0 => {
            let anchor_center_y = a.y + a.height / 2;
            let work_mid_y = work_area.y + work_area.height / 2;
            let work_bottom = work_area.y + work_area.height - margin_px;
            let work_top = work_area.y + margin_px;
            if anchor_center_y <= work_mid_y {
                let top_start = (a.y + a.height + gap_px).max(work_top);
                (work_bottom - top_start).max(1).min(work_max_h)
            } else {
                let bottom_end = (a.y - gap_px).min(work_bottom);
                (bottom_end - work_top).max(1).min(work_max_h)
            }
        }
        _ => work_max_h,
    };

    let pop_w = desired_size.0.min(max_w).max(1);
    let pop_h = desired_size.1.min(max_h).max(1);

    let (x, y) = match anchor {
        Some(a) if a.width > 0 || a.height > 0 => {
            compute_popover_position(a, work_area, (pop_w, pop_h), gap_px, margin_px)
        }
        _ => fallback_popover_position(work_area, (pop_w, pop_h), margin_px),
    };

    (x, y, pop_w, pop_h, max_h)
}

/// Default fallback placement when a tray anchor rect is unavailable (e.g. Linux
/// indicator menu or Wayland window fallback).
pub fn fallback_popover_position(
    work_area: PixelRect,
    popover_size: (i32, i32),
    margin_px: i32,
) -> (i32, i32) {
    let (pop_w, pop_h) = popover_size;
    let x = (work_area.x + work_area.width - pop_w - margin_px).max(work_area.x + margin_px);
    let y = if cfg!(windows) {
        (work_area.y + work_area.height - pop_h - margin_px).max(work_area.y + margin_px)
    } else {
        work_area.y + margin_px
    };
    (x, y)
}

pub fn collect_platform_info(tray_available: bool, daemon_exe: &Path) -> PlatformInfo {
    let app_version = thocci_core::VERSION.to_owned();
    let os = std::env::consts::OS.to_owned();
    let arch = std::env::consts::ARCH.to_owned();
    let daemon_path = daemon_exe.display().to_string();

    #[cfg(target_os = "macos")]
    {
        PlatformInfo {
            app_version,
            os,
            arch,
            session_type: Some("aqua".to_owned()),
            tray_available,
            supports_tray_click_popover: tray_available,
            layer_shell_active: false,
            input_permission_ready: None,
            daemon_exe: daemon_path.clone(),
            permission_title: "macOS Input Monitoring".to_owned(),
            permission_steps: vec![
                "Open System Settings -> Privacy & Security -> Input Monitoring.".to_owned(),
                format!("Enable access for Thocci ({daemon_path})."),
                "Return to Thocci and toggle Enable Thocci. If macOS prompts to quit and reopen, relaunch Thocci for the permission to apply.".to_owned(),
            ],
        }
    }

    #[cfg(target_os = "linux")]
    {
        let session_type = detect_linux_session();
        let input_ready = check_linux_input_access();
        PlatformInfo {
            app_version,
            os,
            arch,
            session_type,
            tray_available,
            supports_tray_click_popover: false,
            layer_shell_active: crate::layer_shell::is_active(),
            input_permission_ready: Some(input_ready),
            daemon_exe: daemon_path,
            permission_title: "Linux keyboard input access".to_owned(),
            permission_steps: vec![
                "Thocci reads physical keystrokes via /dev/input/event* (evdev) on both X11 and Wayland.".to_owned(),
                "Grant access once by adding your user to the input group: sudo usermod -aG input \"$USER\"".to_owned(),
                "Log out and sign back in, then toggle Enable Thocci on the Home screen or run `thocci on`.".to_owned(),
            ],
        }
    }

    #[cfg(windows)]
    {
        PlatformInfo {
            app_version,
            os,
            arch,
            session_type: Some("win32".to_owned()),
            tray_available,
            supports_tray_click_popover: tray_available,
            layer_shell_active: false,
            input_permission_ready: Some(true),
            daemon_exe: daemon_path,
            permission_title: "Windows keyboard hook".to_owned(),
            permission_steps: vec![
                "Thocci captures keystrokes using Windows' interactive-session keyboard hook with no extra setup.".to_owned(),
                "If keystrokes do not sound inside an elevated Administrator window, run Thocci with matching privileges.".to_owned(),
                "Find Thocci in the taskbar notification area (or overflow tray) for Quick Actions.".to_owned(),
            ],
        }
    }
}

/// Register session-only Hyprland window rules before any Thocci window is shown
/// so both the dashboard overlay and Quick Actions popup map floating, borderless,
/// unblurred, and at their exact target dimensions on frame 0 (no tiling flash).
pub fn register_hyprland_window_rules(main_width: u32, main_height: u32) {
    #[cfg(target_os = "linux")]
    {
        if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_none() {
            return;
        }
        let lua = format!(
            concat!(
                r#"hl.window_rule({{ name = "thocci-main-overlay", match = {{ initial_title = "^Thocci$" }}, float = true, size = "{0} {1}", center = true, border_size = 0, no_shadow = true, no_blur = true, rounding = 27, opacity = "1.0 override" }}); "#,
                r#"hl.window_rule({{ name = "thocci-quick-actions-popup", match = {{ initial_title = "^Thocci Quick Actions$" }}, float = true, size = "288 320", min_size = "288 320", max_size = "288 600", border_size = 0, no_shadow = true, no_blur = true, rounding = 28, opacity = "1.0 override" }})"#
            ),
            main_width, main_height
        );
        let _ = std::process::Command::new("hyprctl")
            .args(["eval", &lua])
            .output();
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (main_width, main_height);
    }
}

/// Request floating overlay placement on Linux tiling compositors (Hyprland, Sway/i3)
/// so Thocci overlays active windows instead of splitting/tiling the workspace, while
/// keeping background windows fully interactive and allowing arbitrary user resizing.
pub fn ensure_floating_overlay(
    window_title: &str,
    width: u32,
    height: u32,
    center: bool,
    anchor_to_cursor: bool,
) {
    #[cfg(target_os = "linux")]
    {
        let title = window_title.to_owned();
        let pid = std::process::id();
        std::thread::Builder::new()
            .name("thocci-wm-float".to_owned())
            .spawn(move || {
                if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some() {
                    float_on_hyprland(pid, &title, width, height, center, anchor_to_cursor);
                } else if std::env::var_os("SWAYSOCK").is_some() {
                    float_on_sway(pid, &title, width, height, center);
                }
            })
            .ok();
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (window_title, width, height, center, anchor_to_cursor);
    }
}

#[cfg(target_os = "linux")]
fn float_on_hyprland(
    pid: u32,
    title: &str,
    width: u32,
    height: u32,
    center: bool,
    anchor_to_cursor: bool,
) {
    use std::process::Command;
    use std::time::Duration;

    // Wait briefly for Hyprland to map the newly shown surface.
    let mut matched: Option<(String, bool)> = None;
    for _ in 0..40 {
        let Ok(out) = Command::new("hyprctl").args(["clients", "-j"]).output() else {
            return;
        };
        if out.status.success() {
            if let Ok(clients) = serde_json::from_slice::<Vec<serde_json::Value>>(&out.stdout) {
                for client in clients {
                    let c_pid = client.get("pid").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                    let c_title = client.get("title").and_then(|v| v.as_str()).unwrap_or("");
                    let c_initial = client
                        .get("initialTitle")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if c_pid == pid && (c_title == title || c_initial == title) {
                        if let Some(addr) = client.get("address").and_then(|v| v.as_str()) {
                            let is_floating = client
                                .get("floating")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false);
                            matched = Some((addr.to_owned(), is_floating));
                            break;
                        }
                    }
                }
            }
        }
        if matched.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(15));
    }

    let Some((addr, is_floating)) = matched else {
        return;
    };

    let popup_pos = if anchor_to_cursor {
        compute_hyprland_popup_pos(width as i32, height as i32)
    } else {
        None
    };

    // Hyprland 0.57+ uses Lua dispatchers (`hl.dsp.window.*`); older Hyprland
    // versions use classic string dispatchers (`setfloating`, `resizewindowpixel`).
    // Try Lua first and automatically fall back to classic syntax if rejected.
    let radius = if title == "Thocci Quick Actions" {
        28
    } else {
        27
    };
    let mut lua_cmds = Vec::new();
    if !is_floating {
        lua_cmds.push(format!(
            r#"dispatch hl.dsp.window.float({{ action = "on", window = "address:{addr}" }})"#
        ));
    }
    lua_cmds.push(format!(
        r#"dispatch hl.dsp.window.set_prop({{ prop = "border_size", value = "0", window = "address:{addr}" }})"#
    ));
    lua_cmds.push(format!(
        r#"dispatch hl.dsp.window.set_prop({{ prop = "no_shadow", value = "true", window = "address:{addr}" }})"#
    ));
    lua_cmds.push(format!(
        r#"dispatch hl.dsp.window.set_prop({{ prop = "no_blur", value = "true", window = "address:{addr}" }})"#
    ));
    lua_cmds.push(format!(
        r#"dispatch hl.dsp.window.set_prop({{ prop = "rounding", value = "{radius}", window = "address:{addr}" }})"#
    ));
    lua_cmds.push(format!(
        r#"dispatch hl.dsp.window.resize({{ x = {width}, y = {height}, window = "address:{addr}" }})"#
    ));
    if center {
        lua_cmds.push(format!(
            r#"dispatch hl.dsp.window.center({{ window = "address:{addr}" }})"#
        ));
    } else if let Some((px, py)) = popup_pos {
        lua_cmds.push(format!(
            r#"dispatch hl.dsp.window.move({{ x = {px}, y = {py}, relative = false, window = "address:{addr}" }})"#
        ));
    }
    if center || anchor_to_cursor {
        lua_cmds.push(format!(
            r#"dispatch hl.dsp.focus({{ window = "address:{addr}" }})"#
        ));
    }
    let lua_batch = lua_cmds.join(" ; ");
    let lua_ok = Command::new("hyprctl")
        .args(["--batch", &lua_batch])
        .output()
        .map(|o| {
            o.status.success()
                && !String::from_utf8_lossy(&o.stdout)
                    .to_ascii_lowercase()
                    .contains("error")
        })
        .unwrap_or(false);

    if !lua_ok {
        let mut classic_cmds = Vec::new();
        if !is_floating {
            classic_cmds.push(format!("dispatch setfloating address:{addr}"));
        }
        classic_cmds.push(format!("dispatch setprop address:{addr} noborder 1"));
        classic_cmds.push(format!("dispatch setprop address:{addr} bordersize 0"));
        classic_cmds.push(format!("dispatch setprop address:{addr} noshadow 1"));
        classic_cmds.push(format!("dispatch setprop address:{addr} noblur 1"));
        classic_cmds.push(format!("dispatch setprop address:{addr} rounding {radius}"));
        classic_cmds.push(format!(
            "dispatch resizewindowpixel exact {width} {height},address:{addr}"
        ));
        if center {
            classic_cmds.push(format!("dispatch focuswindow address:{addr}"));
            classic_cmds.push("dispatch centerwindow".to_owned());
        } else if let Some((px, py)) = popup_pos {
            classic_cmds.push(format!(
                "dispatch movewindowpixel exact {px} {py},address:{addr}"
            ));
            classic_cmds.push(format!("dispatch focuswindow address:{addr}"));
        }
        let classic_batch = classic_cmds.join(" ; ");
        let _ = Command::new("hyprctl")
            .args(["--batch", &classic_batch])
            .output();
    }
}

#[cfg(target_os = "linux")]
#[derive(Debug, Clone, Copy)]
pub struct HyprlandBarPopupLayout {
    pub mon_w: i32,
    pub mon_h: i32,
    pub top_inset: i32,
    pub bottom_inset: i32,
    pub local_x: i32,
    pub local_y: i32,
    pub max_h: i32,
    pub anchored_bottom: bool,
}

#[cfg(target_os = "linux")]
struct HyprlandPopupMetrics {
    mon_x: i32,
    mon_y: i32,
    mon_w: i32,
    mon_h: i32,
    top_inset: i32,
    bottom_inset: i32,
    px: i32,
    py: i32,
    max_h: i32,
    anchored_bottom: bool,
}

/// Compute bar-anchored popup layout on Hyprland for a card of size `(pop_w, pop_h)`
/// inside a Wayland layer surface.
#[cfg(target_os = "linux")]
pub fn compute_hyprland_bar_popup_layout(pop_w: i32, pop_h: i32) -> Option<HyprlandBarPopupLayout> {
    let m = compute_hyprland_popup_metrics(pop_w, pop_h)?;
    Some(HyprlandBarPopupLayout {
        mon_w: m.mon_w,
        mon_h: m.mon_h,
        top_inset: m.top_inset,
        bottom_inset: m.bottom_inset,
        local_x: m.px - m.mon_x,
        local_y: m.py - m.mon_y,
        max_h: m.max_h,
        anchored_bottom: m.anchored_bottom,
    })
}

#[cfg(target_os = "linux")]
fn compute_hyprland_popup_pos(pop_w: i32, pop_h: i32) -> Option<(i32, i32)> {
    let m = compute_hyprland_popup_metrics(pop_w, pop_h)?;
    Some((m.px, m.py))
}

#[cfg(target_os = "linux")]
fn compute_hyprland_popup_metrics(pop_w: i32, pop_h: i32) -> Option<HyprlandPopupMetrics> {
    use std::process::Command;

    let cursor_out = Command::new("hyprctl")
        .args(["cursorpos", "-j"])
        .output()
        .ok()?;
    let cursor: serde_json::Value = serde_json::from_slice(&cursor_out.stdout).ok()?;
    let cx = cursor.get("x").and_then(|v| v.as_i64())? as i32;
    let cy = cursor.get("y").and_then(|v| v.as_i64())? as i32;

    let mon_out = Command::new("hyprctl")
        .args(["monitors", "-j"])
        .output()
        .ok()?;
    let monitors: Vec<serde_json::Value> = serde_json::from_slice(&mon_out.stdout).ok()?;
    let mon = monitors
        .iter()
        .find(|m| {
            let mx = m.get("x").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let my = m.get("y").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let scale = m
                .get("scale")
                .and_then(|v| v.as_f64())
                .unwrap_or(1.0)
                .max(0.1);
            let mw =
                (m.get("width").and_then(|v| v.as_f64()).unwrap_or(1920.0) / scale).round() as i32;
            let mh =
                (m.get("height").and_then(|v| v.as_f64()).unwrap_or(1080.0) / scale).round() as i32;
            cx >= mx && cx < mx + mw && cy >= my && cy < my + mh
        })
        .or_else(|| {
            monitors
                .iter()
                .find(|m| m.get("focused").and_then(|v| v.as_bool()).unwrap_or(false))
        })
        .or_else(|| monitors.first())?;

    let mon_name = mon.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let mon_x = mon.get("x").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
    let mon_y = mon.get("y").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
    let scale = mon
        .get("scale")
        .and_then(|v| v.as_f64())
        .unwrap_or(1.0)
        .max(0.1);
    let mon_w =
        (mon.get("width").and_then(|v| v.as_f64()).unwrap_or(1920.0) / scale).round() as i32;
    let mon_h =
        (mon.get("height").and_then(|v| v.as_f64()).unwrap_or(1080.0) / scale).round() as i32;

    let reserved = mon
        .get("reserved")
        .and_then(|v| v.as_array())
        .map(|arr| {
            [
                arr.first().and_then(|v| v.as_i64()).unwrap_or(0) as i32,
                arr.get(1).and_then(|v| v.as_i64()).unwrap_or(0) as i32,
                arr.get(2).and_then(|v| v.as_i64()).unwrap_or(0) as i32,
                arr.get(3).and_then(|v| v.as_i64()).unwrap_or(0) as i32,
            ]
        })
        .unwrap_or([0, 0, 0, 0]);

    let (layer_top, layer_bottom) =
        detect_hyprland_bar_insets(mon_name, mon_x, mon_y, mon_w, mon_h);
    let left_inset = reserved[0];
    let top_inset = reserved[1].max(layer_top);
    let right_inset = reserved[2];
    let bottom_inset = reserved[3].max(layer_bottom);

    let margin = 12;
    let gap = 8;
    let max_h = (mon_h - top_inset - bottom_inset - 2 * gap).max(320);
    let effective_h = pop_h.min(max_h);
    let min_x = mon_x + left_inset + margin;
    let max_x = (mon_x + mon_w - right_inset - pop_w - margin).max(min_x);
    let top_y = mon_y + top_inset + gap;
    let bottom_y = (mon_y + mon_h - bottom_inset - effective_h - gap).max(top_y);

    let (px, py, anchored_bottom) = if cy <= mon_y + top_inset + 180 {
        ((cx - pop_w / 2).clamp(min_x, max_x), top_y, false)
    } else if bottom_inset > 0 && cy >= mon_y + mon_h - bottom_inset - 180 {
        ((cx - pop_w / 2).clamp(min_x, max_x), bottom_y, true)
    } else {
        (max_x, top_y, false)
    };

    Some(HyprlandPopupMetrics {
        mon_x,
        mon_y,
        mon_w,
        mon_h,
        top_inset,
        bottom_inset,
        px,
        py,
        max_h,
        anchored_bottom,
    })
}

#[cfg(target_os = "linux")]
fn detect_hyprland_bar_insets(
    mon_name: &str,
    mon_x: i32,
    mon_y: i32,
    mon_w: i32,
    mon_h: i32,
) -> (i32, i32) {
    use std::process::Command;

    if mon_name.is_empty() {
        return (0, 0);
    }
    let Ok(out) = Command::new("hyprctl").args(["layers", "-j"]).output() else {
        return (0, 0);
    };
    let Ok(root) = serde_json::from_slice::<serde_json::Value>(&out.stdout) else {
        return (0, 0);
    };
    let Some(levels) = root
        .get(mon_name)
        .and_then(|m| m.get("levels"))
        .and_then(|l| l.as_object())
    else {
        return (0, 0);
    };

    let mut top = 0_i32;
    let mut bottom = 0_i32;
    for layers in levels.values() {
        let Some(arr) = layers.as_array() else {
            continue;
        };
        for layer in arr {
            let x = layer.get("x").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let y = layer.get("y").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let w = layer.get("w").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let h = layer.get("h").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            if w >= mon_w / 2 && (20..=96).contains(&h) && x <= mon_x + 4 {
                if y <= mon_y + 4 {
                    top = top.max(h);
                } else if y + h >= mon_y + mon_h - 4 {
                    bottom = bottom.max(h);
                }
            }
        }
    }
    (top, bottom)
}

#[cfg(target_os = "linux")]
fn float_on_sway(pid: u32, title: &str, width: u32, height: u32, center: bool) {
    use std::process::Command;
    use std::time::Duration;

    std::thread::sleep(Duration::from_millis(80));
    let escaped = title.replace('"', "\\\"");
    let move_cmd = if center { ", move position center" } else { "" };
    let rule = format!(
        "[pid={pid} title=\"^{escaped}$\"] floating enable, border none, resize set {width} px {height} px{move_cmd}"
    );
    let _ = Command::new("swaymsg").arg(&rule).output();
}

#[cfg(target_os = "linux")]
fn detect_linux_session() -> Option<String> {
    if let Ok(val) = std::env::var("XDG_SESSION_TYPE") {
        let trimmed = val.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_ascii_lowercase());
        }
    }
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        return Some("wayland".to_owned());
    }
    if std::env::var_os("DISPLAY").is_some() {
        return Some("x11".to_owned());
    }
    None
}

#[cfg(target_os = "linux")]
fn check_linux_input_access() -> bool {
    let Ok(entries) = std::fs::read_dir("/dev/input") else {
        return false;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        let is_event = path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("event"));
        if is_event && std::fs::File::open(&path).is_ok() {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchors_below_top_menu_bar_and_clamps_right_edge() {
        let work = PixelRect {
            x: 0,
            y: 28,
            width: 1440,
            height: 872,
        };
        let tray = PixelRect {
            x: 1380,
            y: 0,
            width: 28,
            height: 28,
        };
        let (x, y) = compute_popover_position(tray, work, (288, 282), 8, 12);
        assert_eq!(x, 1440 - 288 - 12);
        assert_eq!(y, 40);
    }

    #[test]
    fn anchors_above_bottom_windows_taskbar_with_dpi_scaling() {
        // 150% scaling on 1920x1080 -> 288*1.5 = 432, 282*1.5 = 423
        let work = PixelRect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1020,
        };
        let tray = PixelRect {
            x: 1700,
            y: 1028,
            width: 36,
            height: 36,
        };
        let (x, y) = compute_popover_position(tray, work, (432, 423), 12, 18);
        assert_eq!(x, 1920 - 432 - 18);
        assert_eq!(y, 1020 - 423 - 18);
    }

    #[test]
    fn clamps_on_secondary_monitor_with_negative_origin() {
        let work = PixelRect {
            x: -1920,
            y: 0,
            width: 1920,
            height: 1040,
        };
        let tray = PixelRect {
            x: -1910,
            y: 0,
            width: 24,
            height: 24,
        };
        let (x, y) = compute_popover_position(tray, work, (288, 282), 8, 12);
        assert_eq!(x, -1920 + 12);
        assert_eq!(y, 32);
    }

    #[test]
    fn popover_geometry_expands_upward_above_bottom_taskbar_and_clamps_short_work_area() {
        // Bottom taskbar on 1920x1080 @ 150% DPI (work area 1920x1020)
        let work = PixelRect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1020,
        };
        let tray = PixelRect {
            x: 1700,
            y: 1028,
            width: 36,
            height: 36,
        };
        let (_, y_collapsed, w_c, h_c, _) =
            compute_popover_geometry(Some(tray), work, (432, 480), 12, 18);
        let (_, y_expanded, w_e, h_e, _) =
            compute_popover_geometry(Some(tray), work, (432, 882), 12, 18);
        assert_eq!((w_c, h_c), (432, 480));
        assert_eq!((w_e, h_e), (432, 882));
        // Bottom edge stays anchored at the same position above the taskbar.
        assert_eq!(y_collapsed + h_c, y_expanded + h_e);
        assert!(y_expanded >= work.y + 18);

        // Short display work area (height 460px): clamps expanded 588px popover cleanly inside work area
        let short_work = PixelRect {
            x: 0,
            y: 28,
            width: 1280,
            height: 460,
        };
        let top_tray = PixelRect {
            x: 1200,
            y: 0,
            width: 24,
            height: 28,
        };
        let (sx, sy, sw, sh, max_h) =
            compute_popover_geometry(Some(top_tray), short_work, (288, 588), 8, 12);
        assert_eq!(sw, 288);
        assert!(sh <= max_h);
        assert!(sy >= short_work.y + 12);
        assert!(sy + sh <= short_work.y + short_work.height - 12);
        assert!(sx + sw <= short_work.x + short_work.width - 12);
    }
}
