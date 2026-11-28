//! Pure-Rust Wayland Layer Shell (`zwlr_layer_shell_v1`) integration for Linux.
//!
//! Turns Thocci's Dashboard/Onboarding and Quick Actions GTK windows into native
//! Wayland `Overlay` layer surfaces (the exact same protocol used by Quickshell's
//! Settings overlay and Calendar/Wi-Fi/Media popups):
//!
//! - No `xdg_toplevel` is created, so Hyprland workspace window counts stay at 0
//!   and auto-hiding top bars and docks never disappear.
//! - Surfaces sit on `Layer::Overlay` (highest z-index above active apps) with
//!   `exclusive_zone = -1` and `KeyboardInteractivity::OnDemand`, so they never
//!   tile or conflict with bars/docks.
//! - Dashboard/Onboarding uses an exact-sized panel surface at `(x, y, w, h)` so
//!   all background apps, the top bar, and the dock remain directly interactive,
//!   and supports smooth dragging & 8-direction resizing via margin/size updates.
//! - Quick Actions uses a fullscreen transparent overlay with the top bar region
//!   punched out of its input mask (exact `Main.qml` `topBarHole` pattern) so the
//!   top bar remains clickable while clicking anywhere else dismisses the popup.
//! - Automatically falls back to standard windows on X11 or Wayland compositors
//!   that do not advertise `zwlr_layer_shell_v1` (e.g. GNOME/Mutter).

#![cfg(target_os = "linux")]

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use cairo::{RectangleInt, Region};
use gdk::prelude::MonitorExt;
use glib::object::ObjectType;
use gtk::prelude::{GtkWindowExt, WidgetExt};
use serde::Serialize;
use tauri::{AppHandle, Emitter, WebviewWindow, Wry};
use wayland_backend::sys::client::{Backend, ObjectId};
use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle};
use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::{
    self, Layer, ZwlrLayerShellV1,
};
use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1::{
    self, Anchor, KeyboardInteractivity, ZwlrLayerSurfaceV1,
};

static LAYER_SHELL_ACTIVE: AtomicBool = AtomicBool::new(false);

pub fn is_active() -> bool {
    LAYER_SHELL_ACTIVE.load(Ordering::Acquire)
}

/// Silence the harmless upstream GLib deprecation warning emitted by
/// `libayatana-appindicator` when Tauri's tray-icon crate initializes on Linux.
pub fn silence_ayatana_deprecation_warning() {
    glib::log_set_handler(
        Some("libayatana-appindicator"),
        glib::LogLevels::LEVEL_WARNING,
        false,
        false,
        |_, _, _| {},
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerKind {
    /// Exact-sized draggable & resizable overlay panel (Dashboard / Onboarding).
    FloatingPanel,
    /// Full-output transparent popup backdrop with top-bar input hole (Quick Actions).
    BarPopup,
}

#[derive(Debug, Clone, Copy)]
pub struct PanelRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

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
    #[serde(rename = "maxHeight")]
    pub max_height: i32,
    pub animate: bool,
}

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

#[derive(Debug, Clone)]
struct SurfaceSpec {
    namespace: String,
    kind: LayerKind,
    rect: PanelRect,
    screen_width: i32,
    screen_height: i32,
    /// Temporarily expands `FloatingPanel` input mask to full screen during active drag/resize.
    input_fullscreen: bool,
    /// Only used for `BarPopup`: top bar height punched out of the input region.
    top_bar_hole_height: i32,
    /// Only used for `BarPopup`: bottom bar height punched out of the input region.
    bottom_bar_hole_height: i32,
    /// Only used for `BarPopup`: `(x, y)` of the popup card inside the overlay.
    popup_card_pos: (i32, i32),
    /// Only used for `BarPopup`: `(w, h)` of the popup card inside the overlay.
    popup_card_size: (i32, i32),
    /// Only used for `BarPopup`: whether the popup is anchored above a bottom panel.
    popup_anchored_bottom: bool,
    /// Only used for `BarPopup`: maximum logical height that fits in the monitor work area.
    popup_max_height: i32,
}

struct ActiveSurface {
    layer_surface: ZwlrLayerSurfaceV1,
    wl_surface: WlSurface,
    configured_width: u32,
    configured_height: u32,
}

struct LayerDispatchState {
    layer_shell: Option<ZwlrLayerShellV1>,
    pending_configures: HashMap<String, (u32, u32)>,
    closed_labels: Vec<String>,
}

impl Dispatch<WlRegistry, ()> for LayerDispatchState {
    fn event(
        state: &mut Self,
        registry: &WlRegistry,
        event: wl_registry::Event,
        _data: &(),
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            if interface == ZwlrLayerShellV1::interface().name && state.layer_shell.is_none() {
                let bound = registry.bind::<ZwlrLayerShellV1, _, _>(name, version.min(4), qh, ());
                state.layer_shell = Some(bound);
            }
        }
    }
}

impl Dispatch<ZwlrLayerShellV1, ()> for LayerDispatchState {
    fn event(
        _state: &mut Self,
        _proxy: &ZwlrLayerShellV1,
        _event: zwlr_layer_shell_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrLayerSurfaceV1, String> for LayerDispatchState {
    fn event(
        state: &mut Self,
        layer_surface: &ZwlrLayerSurfaceV1,
        event: zwlr_layer_surface_v1::Event,
        label: &String,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_layer_surface_v1::Event::Configure {
                serial,
                width,
                height,
            } => {
                layer_surface.ack_configure(serial);
                state
                    .pending_configures
                    .insert(label.clone(), (width, height));
            }
            zwlr_layer_surface_v1::Event::Closed => {
                state.closed_labels.push(label.clone());
            }
            _ => {}
        }
    }
}

struct LayerShellRuntime {
    conn: Connection,
    event_queue: EventQueue<LayerDispatchState>,
    state: LayerDispatchState,
    specs: HashMap<String, SurfaceSpec>,
    active: HashMap<String, ActiveSurface>,
    windows: HashMap<String, gtk::ApplicationWindow>,
    app_handle: Option<AppHandle<Wry>>,
    pump_installed: bool,
}

thread_local! {
    static LAYER_RUNTIME: RefCell<Option<LayerShellRuntime>> = const { RefCell::new(None) };
}

fn ensure_runtime(app: &AppHandle<Wry>, display: &gdk::Display) -> bool {
    LAYER_RUNTIME.with(|cell| {
        if cell.borrow().is_some() {
            return true;
        }

        let display_ptr = display.as_ptr();
        let is_wayland = unsafe {
            glib::gobject_ffi::g_type_check_instance_is_a(
                display_ptr as *mut glib::gobject_ffi::GTypeInstance,
                gdk_wayland_sys::gdk_wayland_display_get_type(),
            ) != 0
        };
        if !is_wayland {
            return false;
        }

        let wl_display = unsafe {
            gdk_wayland_sys::gdk_wayland_display_get_wl_display(
                display_ptr as *mut gdk_wayland_sys::GdkWaylandDisplay,
            )
        };
        if wl_display.is_null() {
            return false;
        }

        let backend = unsafe { Backend::from_foreign_display(wl_display as *mut _) };
        let conn = Connection::from_backend(backend);
        let mut event_queue = conn.new_event_queue::<LayerDispatchState>();
        let qh = event_queue.handle();
        let wl_display_proxy = conn.display();
        let _registry = wl_display_proxy.get_registry(&qh, ());

        let mut state = LayerDispatchState {
            layer_shell: None,
            pending_configures: HashMap::new(),
            closed_labels: Vec::new(),
        };
        if event_queue.roundtrip(&mut state).is_err() || state.layer_shell.is_none() {
            log::info!("zwlr_layer_shell_v1 unavailable on this Wayland compositor; using standard floating windows");
            return false;
        }

        log::info!("Wayland zwlr_layer_shell_v1 initialized for Thocci overlay surfaces");
        LAYER_SHELL_ACTIVE.store(true, Ordering::Release);
        *cell.borrow_mut() = Some(LayerShellRuntime {
            conn,
            event_queue,
            state,
            specs: HashMap::new(),
            active: HashMap::new(),
            windows: HashMap::new(),
            app_handle: Some(app.clone()),
            pump_installed: false,
        });
        install_event_pump();
        true
    })
}

fn install_event_pump() {
    let should_install = LAYER_RUNTIME.with(|cell| {
        let mut borrow = cell.borrow_mut();
        if let Some(rt) = borrow.as_mut() {
            if !rt.pump_installed {
                rt.pump_installed = true;
                return true;
            }
        }
        false
    });
    if !should_install {
        return;
    }

    // Low-frequency fallback pump for compositor-initiated Close/Configure events
    // (explicit show/resize paths roundtrip and flush immediately on demand).
    glib::timeout_add_local(Duration::from_millis(250), || {
        let to_hide = LAYER_RUNTIME.with(|cell| {
            let mut borrow = cell.borrow_mut();
            let Some(rt) = borrow.as_mut() else {
                return Vec::new();
            };
            if rt.active.is_empty() {
                return Vec::new();
            }
            let _ = rt.event_queue.dispatch_pending(&mut rt.state);
            process_pending_events(rt)
        });
        for gtk_win in to_hide {
            gtk_win.hide();
        }
        glib::ControlFlow::Continue
    });
}

fn process_pending_events(rt: &mut LayerShellRuntime) -> Vec<gtk::ApplicationWindow> {
    if rt.state.pending_configures.is_empty() && rt.state.closed_labels.is_empty() {
        return Vec::new();
    }
    let configures: Vec<(String, (u32, u32))> = rt.state.pending_configures.drain().collect();
    for (label, (w, h)) in configures {
        if let Some(active) = rt.active.get_mut(&label) {
            if w > 0 {
                active.configured_width = w;
            }
            if h > 0 {
                active.configured_height = h;
            }
            let target_w = active.configured_width;
            let target_h = active.configured_height;
            if let Some(gtk_win) = rt.windows.get(&label) {
                if target_w > 0 && target_h > 0 {
                    gtk_win.set_size_request(target_w as i32, target_h as i32);
                    gtk_win.resize(target_w as i32, target_h as i32);
                }
                if let Some(spec) = rt.specs.get_mut(&label) {
                    if target_w > 0 && target_h > 0 {
                        spec.screen_width = target_w as i32;
                        spec.screen_height = target_h as i32;
                    }
                    match spec.kind {
                        LayerKind::FloatingPanel => {
                            if spec.input_fullscreen {
                                apply_fullscreen_input_mask(
                                    gtk_win,
                                    spec.screen_width,
                                    spec.screen_height,
                                );
                            } else {
                                apply_panel_input_mask(gtk_win, spec.rect);
                            }
                            if let Some(app) = rt.app_handle.as_ref() {
                                let _ = app.emit_to(
                                    &label,
                                    "thocci://overlay-placement",
                                    OverlayPlacement {
                                        layer_shell: true,
                                        x: spec.rect.x,
                                        y: spec.rect.y,
                                        width: spec.rect.width,
                                        height: spec.rect.height,
                                        screen_width: spec.screen_width,
                                        screen_height: spec.screen_height,
                                    },
                                );
                            }
                        }
                        LayerKind::BarPopup => {
                            apply_bar_popup_input_mask(
                                gtk_win,
                                target_w as i32,
                                target_h as i32,
                                spec.top_bar_hole_height,
                                spec.bottom_bar_hole_height,
                            );
                            if let Some(app) = rt.app_handle.as_ref() {
                                let _ = app.emit_to(
                                    "quick-actions",
                                    "thocci://quick-actions-placement",
                                    QuickActionsPlacement {
                                        layer_shell: true,
                                        x: spec.popup_card_pos.0,
                                        y: spec.popup_card_pos.1,
                                        screen_width: target_w as i32,
                                        screen_height: target_h as i32,
                                        max_height: spec.popup_max_height,
                                        animate: false,
                                    },
                                );
                            }
                        }
                    }
                }
            }
            active.wl_surface.commit();
        }
    }
    let mut to_hide = Vec::new();
    let closed = std::mem::take(&mut rt.state.closed_labels);
    for label in closed {
        if let Some(active) = rt.active.remove(&label) {
            active.layer_surface.destroy();
            let _ = rt.conn.flush();
        }
        if let Some(gtk_win) = rt.windows.get(&label) {
            to_hide.push(gtk_win.clone());
        }
    }
    let _ = rt.conn.flush();
    to_hide
}

fn mark_custom_surface(gtk_win: &gtk::ApplicationWindow) {
    if let Some(gdk_win) = gtk_win.window() {
        unsafe {
            gdk_wayland_sys::gdk_wayland_window_set_use_custom_surface(
                gdk_win.as_ptr() as *mut gdk_wayland_sys::GdkWaylandWindow
            );
        }
    }
}

fn attach_layer_surface_for(label: &str) {
    LAYER_RUNTIME.with(|cell| {
        let mut borrow = cell.borrow_mut();
        let Some(rt) = borrow.as_mut() else {
            return;
        };
        let Some(gtk_win) = rt.windows.get(label).cloned() else {
            return;
        };
        let Some(spec) = rt.specs.get(label).cloned() else {
            return;
        };
        let Some(layer_shell) = rt.state.layer_shell.clone() else {
            return;
        };

        // Tear down any previous role before attaching a new one.
        if let Some(prev) = rt.active.remove(label) {
            prev.layer_surface.destroy();
        }

        let Some(gdk_win) = gtk_win.window() else {
            return;
        };
        let wl_surface_ptr = unsafe {
            gdk_wayland_sys::gdk_wayland_window_get_wl_surface(
                gdk_win.as_ptr() as *mut gdk_wayland_sys::GdkWaylandWindow
            )
        };
        if wl_surface_ptr.is_null() {
            log::warn!("layer_shell: wl_surface null on map for '{label}'");
            return;
        }

        let Ok(obj_id) =
            (unsafe { ObjectId::from_ptr(WlSurface::interface(), wl_surface_ptr as *mut _) })
        else {
            log::warn!("layer_shell: failed to wrap wl_surface for '{label}'");
            return;
        };
        let Ok(wl_surface) = WlSurface::from_id(&rt.conn, obj_id) else {
            log::warn!("layer_shell: WlSurface::from_id failed for '{label}'");
            return;
        };

        let qh = rt.event_queue.handle();
        let layer_surface = layer_shell.get_layer_surface(
            &wl_surface,
            None,
            Layer::Overlay,
            spec.namespace.clone(),
            &qh,
            label.to_owned(),
        );

        // Both `FloatingPanel` (Dashboard/Onboarding) and `BarPopup` (Quick Actions)
        // anchor to all four screen edges at `(0, 0)` just like Quickshell's `Main.qml`
        // and restrict pointer input via `input_shape_combine_region`. Because the
        // layer surface itself never moves on the compositor, dragging and resizing
        // inside the webview runs at native 60/120fps with zero compositor jitter.
        layer_surface.set_exclusive_zone(-1);
        layer_surface.set_keyboard_interactivity(KeyboardInteractivity::OnDemand);
        layer_surface.set_anchor(Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right);
        layer_surface.set_size(0, 0);
        layer_surface.set_margin(0, 0, 0, 0);

        wl_surface.commit();

        rt.active.insert(
            label.to_owned(),
            ActiveSurface {
                layer_surface,
                wl_surface,
                configured_width: spec.screen_width.max(0) as u32,
                configured_height: spec.screen_height.max(0) as u32,
            },
        );

        let _ = rt.event_queue.roundtrip(&mut rt.state);
        let _ = process_pending_events(rt);
    });
}

fn detach_layer_surface_for(label: &str) {
    LAYER_RUNTIME.with(|cell| {
        let mut borrow = cell.borrow_mut();
        let Some(rt) = borrow.as_mut() else {
            return;
        };
        if let Some(active) = rt.active.remove(label) {
            active.layer_surface.destroy();
            let _ = rt.conn.flush();
        }
    });
}

/// Safely hide a layer-shell window by destroying its `zwlr_layer_surface_v1`
/// role object *before* GTK unmaps and destroys the underlying `wl_surface`.
pub fn hide_window(label: &str) {
    detach_layer_surface_for(label);
    let gtk_win = LAYER_RUNTIME.with(|cell| {
        cell.borrow()
            .as_ref()
            .and_then(|rt| rt.windows.get(label).cloned())
    });
    if let Some(gtk_win) = gtk_win {
        gtk_win.hide();
    }
}

fn apply_panel_input_mask(gtk_win: &gtk::ApplicationWindow, rect: PanelRect) {
    let Some(gdk_win) = gtk_win.window() else {
        return;
    };
    if rect.width <= 0 || rect.height <= 0 {
        return;
    }
    let region =
        Region::create_rectangle(&RectangleInt::new(rect.x, rect.y, rect.width, rect.height));
    gdk_win.input_shape_combine_region(&region, 0, 0);
}

fn apply_fullscreen_input_mask(gtk_win: &gtk::ApplicationWindow, screen_w: i32, screen_h: i32) {
    let Some(gdk_win) = gtk_win.window() else {
        return;
    };
    if screen_w <= 0 || screen_h <= 0 {
        return;
    }
    let region = Region::create_rectangle(&RectangleInt::new(0, 0, screen_w, screen_h));
    gdk_win.input_shape_combine_region(&region, 0, 0);
}

fn apply_bar_popup_input_mask(
    gtk_win: &gtk::ApplicationWindow,
    screen_w: i32,
    screen_h: i32,
    top_bar_h: i32,
    bottom_bar_h: i32,
) {
    let Some(gdk_win) = gtk_win.window() else {
        return;
    };
    if screen_w <= 0 || screen_h <= 0 {
        return;
    }
    let region = Region::create_rectangle(&RectangleInt::new(0, 0, screen_w, screen_h));
    if top_bar_h > 0 && top_bar_h < screen_h {
        let _ = region.subtract_rectangle(&RectangleInt::new(0, 0, screen_w, top_bar_h));
    }
    if bottom_bar_h > 0 && bottom_bar_h < screen_h {
        let _ = region.subtract_rectangle(&RectangleInt::new(
            0,
            (screen_h - bottom_bar_h).max(0),
            screen_w,
            bottom_bar_h,
        ));
    }
    gdk_win.input_shape_combine_region(&region, 0, 0);
}

/// Attach Wayland layer-shell management to a hidden Tauri window before its
/// first `show()`. Returns `true` if `zwlr_layer_shell_v1` is active.
pub fn setup_window(
    app: &AppHandle<Wry>,
    win: &WebviewWindow<Wry>,
    namespace: &str,
    kind: LayerKind,
    initial_rect: PanelRect,
) -> bool {
    let Ok(gtk_win) = win.gtk_window() else {
        return false;
    };
    let display = gtk_win.display();
    if !ensure_runtime(app, &display) {
        return false;
    }

    let label = win.label().to_owned();
    let (mon_w, mon_h) = current_monitor_logical_size().unwrap_or((1920, 1080));
    gtk_win.set_decorated(false);
    gtk_win.set_app_paintable(true);

    if gtk_win.is_realized() {
        mark_custom_surface(&gtk_win);
    } else {
        gtk_win.realize();
        mark_custom_surface(&gtk_win);
    }

    LAYER_RUNTIME.with(|cell| {
        if let Some(rt) = cell.borrow_mut().as_mut() {
            rt.specs.insert(
                label.clone(),
                SurfaceSpec {
                    namespace: namespace.to_owned(),
                    kind,
                    rect: initial_rect,
                    screen_width: mon_w,
                    screen_height: mon_h,
                    input_fullscreen: false,
                    top_bar_hole_height: 0,
                    bottom_bar_hole_height: 0,
                    popup_card_pos: (initial_rect.x, initial_rect.y),
                    popup_card_size: (initial_rect.width, initial_rect.height),
                    popup_anchored_bottom: false,
                    popup_max_height: (mon_h - 60).max(320),
                },
            );
            rt.windows.insert(label.clone(), gtk_win.clone());
        }
    });

    gtk_win.connect_realize(|w| {
        mark_custom_surface(w);
    });

    let map_label = label.clone();
    gtk_win.connect_map(move |_| {
        attach_layer_surface_for(&map_label);
    });

    // Clear stale tracking if GTK unmaps externally without going through `hide_window`.
    let unmap_label = label;
    gtk_win.connect_unmap(move |_| {
        LAYER_RUNTIME.with(|cell| {
            if let Ok(mut guard) = cell.try_borrow_mut() {
                if let Some(rt) = guard.as_mut() {
                    let _ = rt.active.remove(&unmap_label);
                }
            }
        });
    });

    true
}

/// Update the stored card rect and Wayland input mask for a `FloatingPanel`.
/// When `emit_to_webview` is `true`, notifies the webview of the updated geometry
/// (e.g. after Setup guide size changes or mode transitions).
pub fn set_panel_rect(label: &str, rect: PanelRect, emit_to_webview: bool) {
    LAYER_RUNTIME.with(|cell| {
        let mut borrow = cell.borrow_mut();
        let Some(rt) = borrow.as_mut() else {
            return;
        };
        let mut placement_to_emit = None;
        if let Some(spec) = rt.specs.get_mut(label) {
            spec.rect = rect;
            spec.input_fullscreen = false;
            if emit_to_webview {
                placement_to_emit = Some(OverlayPlacement {
                    layer_shell: true,
                    x: rect.x,
                    y: rect.y,
                    width: rect.width,
                    height: rect.height,
                    screen_width: spec.screen_width,
                    screen_height: spec.screen_height,
                });
            }
        }
        if let Some(gtk_win) = rt.windows.get(label) {
            apply_panel_input_mask(gtk_win, rect);
        }
        if let Some(active) = rt.active.get(label) {
            active.wl_surface.commit();
            let _ = rt.conn.flush();
        }
        if let (Some(app), Some(placement)) = (rt.app_handle.as_ref(), placement_to_emit) {
            let _ = app.emit_to(label, "thocci://overlay-placement", &placement);
        }
    });
}

/// Temporarily demote a `FloatingPanel` behind normal windows and disable its
/// input/focus while a native modal file dialog is open, then restore it on
/// `Overlay` once the dialog closes.
pub fn set_panel_modal_dialog_open(label: &str, open: bool) {
    LAYER_RUNTIME.with(|cell| {
        let mut borrow = cell.borrow_mut();
        let Some(rt) = borrow.as_mut() else {
            return;
        };
        let rect = match rt.specs.get_mut(label) {
            Some(spec) => {
                if !open {
                    spec.input_fullscreen = false;
                }
                spec.rect
            }
            None => return,
        };
        if let Some(gtk_win) = rt.windows.get(label) {
            if open {
                if let Some(gdk_win) = gtk_win.window() {
                    let empty = Region::create();
                    gdk_win.input_shape_combine_region(&empty, 0, 0);
                }
            } else {
                apply_panel_input_mask(gtk_win, rect);
            }
        }
        if let Some(active) = rt.active.get(label) {
            if open {
                active.layer_surface.set_layer(Layer::Bottom);
                active
                    .layer_surface
                    .set_keyboard_interactivity(KeyboardInteractivity::None);
            } else {
                active.layer_surface.set_layer(Layer::Overlay);
                active
                    .layer_surface
                    .set_keyboard_interactivity(KeyboardInteractivity::OnDemand);
            }
            active.wl_surface.commit();
            let _ = rt.conn.flush();
        }
    });
}

/// Temporarily expand or restore the `FloatingPanel` input mask during an active
/// pointer drag or resize so fast cursor motion never leaves the input surface.
pub fn set_panel_input_fullscreen(label: &str, fullscreen: bool) {
    LAYER_RUNTIME.with(|cell| {
        let mut borrow = cell.borrow_mut();
        let Some(rt) = borrow.as_mut() else {
            return;
        };
        let (rect, sw, sh) = match rt.specs.get_mut(label) {
            Some(spec) => {
                spec.input_fullscreen = fullscreen;
                (spec.rect, spec.screen_width, spec.screen_height)
            }
            None => return,
        };
        if let Some(gtk_win) = rt.windows.get(label) {
            if fullscreen {
                apply_fullscreen_input_mask(gtk_win, sw, sh);
            } else {
                apply_panel_input_mask(gtk_win, rect);
            }
        }
        if let Some(active) = rt.active.get(label) {
            active.wl_surface.commit();
            let _ = rt.conn.flush();
        }
    });
}

/// Read the current `OverlayPlacement` for a `FloatingPanel`.
pub fn get_panel_placement(label: &str) -> Option<OverlayPlacement> {
    LAYER_RUNTIME.with(|cell| {
        cell.borrow().as_ref().and_then(|rt| {
            rt.specs.get(label).map(|s| OverlayPlacement {
                layer_shell: true,
                x: s.rect.x,
                y: s.rect.y,
                width: s.rect.width,
                height: s.rect.height,
                screen_width: s.screen_width,
                screen_height: s.screen_height,
            })
        })
    })
}

/// Center a `FloatingPanel` on the active monitor with dimensions `(width, height)`.
pub fn center_panel(label: &str, width: i32, height: i32) -> PanelRect {
    let (mon_w, mon_h) = current_monitor_logical_size().unwrap_or((1920, 1080));
    let x = ((mon_w - width) / 2).max(0);
    let y = ((mon_h - height) / 2).max(0);
    let rect = PanelRect {
        x,
        y,
        width,
        height,
    };
    set_panel_rect(label, rect, true);
    rect
}

/// Prepare Quick Actions `BarPopup` placement and bar input holes before showing.
pub fn prepare_bar_popup(label: &str, card_w: i32, card_h: i32) -> QuickActionsPlacement {
    let layout = compute_bar_popup_geometry(card_w, card_h);
    let clamped_h = card_h.min(layout.max_h).max(320);
    let placement = QuickActionsPlacement {
        layer_shell: true,
        x: layout.local_x,
        y: layout.local_y,
        screen_width: layout.mon_w,
        screen_height: layout.mon_h,
        max_height: layout.max_h,
        animate: true,
    };

    LAYER_RUNTIME.with(|cell| {
        let mut borrow = cell.borrow_mut();
        let Some(rt) = borrow.as_mut() else {
            return;
        };
        if let Some(spec) = rt.specs.get_mut(label) {
            spec.top_bar_hole_height = layout.top_inset;
            spec.bottom_bar_hole_height = layout.bottom_inset;
            spec.popup_card_pos = (layout.local_x, layout.local_y);
            spec.popup_card_size = (card_w, clamped_h);
            spec.popup_anchored_bottom = layout.anchored_bottom;
            spec.popup_max_height = layout.max_h;
            spec.rect = PanelRect {
                x: 0,
                y: 0,
                width: layout.mon_w,
                height: layout.mon_h,
            };
        }
        if let Some(gtk_win) = rt.windows.get(label) {
            apply_bar_popup_input_mask(
                gtk_win,
                layout.mon_w,
                layout.mon_h,
                layout.top_inset,
                layout.bottom_inset,
            );
        }
    });

    placement
}

/// Resize an already-placed Quick Actions `BarPopup` while preserving its monitor
/// and top/bottom panel anchor. Clamps height inside the monitor work area and
/// updates the Wayland surface cleanly.
pub fn resize_bar_popup(
    label: &str,
    card_w: i32,
    card_h: i32,
) -> Option<(QuickActionsPlacement, i32)> {
    LAYER_RUNTIME.with(|cell| {
        let mut borrow = cell.borrow_mut();
        let rt = borrow.as_mut()?;
        let spec = rt.specs.get_mut(label)?;
        let gap = 8;
        let max_h =
            (spec.screen_height - spec.top_bar_hole_height - spec.bottom_bar_hole_height - 2 * gap)
                .max(320);
        let clamped_h = card_h.clamp(320, max_h);
        let top_y = spec.top_bar_hole_height + gap;
        let max_y = (spec.screen_height - spec.bottom_bar_hole_height - clamped_h - gap).max(top_y);
        let next_y = if spec.popup_anchored_bottom {
            max_y
        } else {
            spec.popup_card_pos.1.clamp(top_y, max_y)
        };
        let max_x = (spec.screen_width - card_w - 12).max(12);
        let next_x = spec.popup_card_pos.0.clamp(12, max_x);

        spec.popup_card_pos = (next_x, next_y);
        spec.popup_card_size = (card_w, clamped_h);
        spec.popup_max_height = max_h;

        let placement = QuickActionsPlacement {
            layer_shell: true,
            x: next_x,
            y: next_y,
            screen_width: spec.screen_width,
            screen_height: spec.screen_height,
            max_height: max_h,
            animate: false,
        };
        if let Some(gtk_win) = rt.windows.get(label) {
            apply_bar_popup_input_mask(
                gtk_win,
                spec.screen_width,
                spec.screen_height,
                spec.top_bar_hole_height,
                spec.bottom_bar_hole_height,
            );
        }
        if let Some(active) = rt.active.get(label) {
            active.wl_surface.commit();
            let _ = rt.conn.flush();
        }
        Some((placement, clamped_h))
    })
}

/// Query logical `(width, height)` of the focused monitor via GDK (works on all
/// Wayland compositors without requiring Hyprland).
pub fn current_monitor_logical_size() -> Option<(i32, i32)> {
    // Prefer Hyprland's focused monitor if available (GDK primary_monitor can lag
    // on multi-monitor Hyprland).
    if let Some(layout) = query_hyprland_focused_monitor() {
        return Some((layout.mon_w, layout.mon_h));
    }
    let display = gdk::Display::default()?;
    let monitor = display.primary_monitor().or_else(|| display.monitor(0))?;
    let geom = monitor.geometry();
    Some((geom.width(), geom.height()))
}

fn compute_bar_popup_geometry(card_w: i32, card_h: i32) -> crate::platform::HyprlandBarPopupLayout {
    if let Some(layout) = crate::platform::compute_hyprland_bar_popup_layout(card_w, card_h) {
        return layout;
    }
    let (mon_w, mon_h) = current_monitor_logical_size().unwrap_or((1920, 1080));
    let top_inset = 44;
    let bottom_inset = 0;
    let gap = 8;
    let max_h = (mon_h - top_inset - bottom_inset - 2 * gap).max(320);
    let local_x = (mon_w - card_w - 12).max(12);
    let local_y = top_inset + gap;
    crate::platform::HyprlandBarPopupLayout {
        mon_w,
        mon_h,
        top_inset,
        bottom_inset,
        local_x,
        local_y,
        max_h,
        anchored_bottom: false,
    }
}

fn query_hyprland_focused_monitor() -> Option<crate::platform::HyprlandBarPopupLayout> {
    std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE")?;
    crate::platform::compute_hyprland_bar_popup_layout(288, 320)
}
