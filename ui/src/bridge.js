/**
 * Tauri IPC bridge with graceful in-browser preview fallback.
 */

function tauriInvoke() {
  return window.__TAURI__?.core?.invoke ?? null;
}

function tauriListen() {
  return window.__TAURI__?.event?.listen ?? null;
}

export function isTauriRuntime() {
  return typeof tauriInvoke() === "function";
}

const MOCK_PINS_KEY = "thocci:preview:pinned_packs";

function previewStorage() {
  if (typeof document === "undefined") return null;
  try {
    return globalThis.localStorage ?? null;
  } catch {
    return null;
  }
}

function loadMockPins() {
  try {
    const raw = previewStorage()?.getItem(MOCK_PINS_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    return Array.isArray(parsed) ? parsed.filter((id) => typeof id === "string") : [];
  } catch {
    return [];
  }
}

function saveMockPins(pins) {
  try {
    previewStorage()?.setItem(MOCK_PINS_KEY, JSON.stringify(pins));
  } catch {
    // Non-browser test environments have no localStorage.
  }
}

// Local mock state used only when previewing ui/index.html outside Tauri.
const mockState = {
  status: {
    running: false,
    responsive: false,
    pid: null,
    volume: 55,
    pack: "creamy",
    stereo: false,
    saved_volume: 55,
    saved_pack: "creamy",
    saved_stereo: false,
  },
  prefs: {
    onboarding_completed: true,
    theme: null,
    animate_mascot: true,
    window_width: null,
    window_height: null,
    pinned_packs: loadMockPins(),
  },
  catalog: [
    { id: "cherry-mx", display_name: "Cherry MX Black", subtitle: "Linear · ABS keycaps", raw_name: "CherryMX Black - ABS keycaps", origin: "builtin", is_default: true },
    { id: "banana-split", display_name: "Banana Split", subtitle: "Lubed linear", raw_name: "Banana Split Lubed by Akira", origin: "builtin", is_default: false },
    { id: "creamy", display_name: "Creamy", subtitle: "Soft and muted", raw_name: "creamy (still getting better)", origin: "builtin", is_default: false },
    { id: "wooden-keyboard", display_name: "Wooden Keyboard", subtitle: "Dry and hollow", raw_name: "Wooden Keyboard", origin: "builtin", is_default: false },
    { id: "typewriter", display_name: "Typewriter", subtitle: "Classic mechanical", raw_name: "TypeWriter", origin: "builtin", is_default: false },
    { id: "alpaca", display_name: "Durock Alpaca", subtitle: "Built-in soundpack", raw_name: "Durock Alpaca", origin: "builtin", source: "kbsim", is_default: false },
    { id: "cherrymx-blue-pbt", display_name: "CherryMX Blue - PBT keycaps", subtitle: "Built-in soundpack", raw_name: "CherryMX Blue - PBT keycaps", origin: "builtin", source: "Mechvibes", is_default: false },
  ],
};

async function call(cmd, args = {}) {
  const invoke = tauriInvoke();
  if (invoke) {
    return invoke(cmd, args);
  }
  switch (cmd) {
    case "get_status":
      return { ...mockState.status };
    case "list_pack_catalog":
      return [...mockState.catalog];
    case "toggle_daemon":
      mockState.status.running = !mockState.status.running;
      mockState.status.responsive = mockState.status.running;
      return { ...mockState.status };
    case "start_daemon":
      mockState.status.running = true;
      mockState.status.responsive = true;
      return { ...mockState.status };
    case "stop_daemon":
      mockState.status.running = false;
      mockState.status.responsive = false;
      return { ...mockState.status };
    case "set_volume": {
      const clamped = Math.max(0, Math.min(100, Math.round(Number(args.level) || 0)));
      mockState.status.volume = clamped;
      mockState.status.saved_volume = clamped;
      return { ...mockState.status };
    }
    case "set_pack":
      mockState.status.pack = args.packId;
      mockState.status.saved_pack = args.packId;
      return { ...mockState.status };
    case "set_stereo": {
      const enabled = Boolean(args.enabled);
      mockState.status.stereo = enabled;
      mockState.status.saved_stereo = enabled;
      return { ...mockState.status };
    }
    case "get_ui_prefs":
      return { ...mockState.prefs, pinned_packs: [...mockState.prefs.pinned_packs] };
    case "save_ui_prefs":
      mockState.prefs = { ...mockState.prefs, ...args.prefsUpdate };
      return { ...mockState.prefs, pinned_packs: [...mockState.prefs.pinned_packs] };
    case "set_pack_pinned": {
      const id = String(args.packId ?? "").trim();
      const pinned = Boolean(args.pinned);
      const list = [...mockState.prefs.pinned_packs];
      if (!pinned) {
        const idx = list.indexOf(id);
        if (idx === -1) {
          return { prefs: { ...mockState.prefs, pinned_packs: list }, status: "unchanged" };
        }
        list.splice(idx, 1);
        mockState.prefs.pinned_packs = list;
        saveMockPins(list);
        return { prefs: { ...mockState.prefs, pinned_packs: [...list] }, status: "unpinned" };
      }
      if (!mockState.catalog.some((p) => p.id === id)) {
        throw new Error(`Unknown soundpack '${id}'.`);
      }
      if (list.includes(id)) {
        return { prefs: { ...mockState.prefs, pinned_packs: list }, status: "unchanged" };
      }
      if (list.length >= 10) {
        return { prefs: { ...mockState.prefs, pinned_packs: list }, status: "limit" };
      }
      list.push(id);
      mockState.prefs.pinned_packs = list;
      saveMockPins(list);
      return { prefs: { ...mockState.prefs, pinned_packs: [...list] }, status: "pinned" };
    }
    case "set_window_size":
      mockState.prefs = {
        ...mockState.prefs,
        window_width: args.width ?? null,
        window_height: args.height ?? null,
      };
      return { ...mockState.prefs, pinned_packs: [...mockState.prefs.pinned_packs] };
    case "complete_onboarding":
      mockState.prefs.onboarding_completed = true;
      return { ...mockState.prefs, pinned_packs: [...mockState.prefs.pinned_packs] };
    case "play_preview":
      return Boolean(mockState.status.running && mockState.status.responsive && mockState.status.volume > 0);
    case "set_quick_actions_height":
      return Math.max(320, Math.min(600, Math.round(Number(args.height) || 320)));
    case "get_platform_info":
      return {
        app_version: "1.1.1",
        os: "linux",
        arch: "x86_64",
        session_type: "browser-preview",
        tray_available: false,
        supports_tray_click_popover: false,
        layer_shell_active: false,
        input_permission_ready: true,
        daemon_exe: "thocci",
        permission_title: "Desktop engine preview",
        permission_steps: ["Launch via `npm run dev` or the installed Thocci desktop package to connect to the Rust audio engine."],
      };
    case "get_overlay_placement":
    case "set_overlay_input_fullscreen":
    case "set_overlay_rect":
    case "get_quick_actions_placement":
      return null;
    case "pick_and_import_wav":
      return null;
    case "check_for_update":
      return {
        available: false,
        currentVersion: "1.1.1",
        version: null,
        notes: null,
      };
    case "install_update":
      return null;
    default:
      return null;
  }
}

export const api = {
  getStatus: () => call("get_status"),
  listPackCatalog: () => call("list_pack_catalog"),
  startDaemon: (volume = null) => call("start_daemon", { volume }),
  stopDaemon: () => call("stop_daemon"),
  toggleDaemon: () => call("toggle_daemon"),
  setVolume: (level) => call("set_volume", { level }),
  setPack: (packId) => call("set_pack", { packId }),
  setStereo: (enabled) => call("set_stereo", { enabled }),
  pickAndImportWav: () => call("pick_and_import_wav"),
  importWavPack: (path, name = null) => call("import_wav_pack", { path, name }),
  playPreview: (code = null, kind = "down") => call("play_preview", { code, kind }),
  getUiPrefs: () => call("get_ui_prefs"),
  saveUiPrefs: (prefsUpdate) => call("save_ui_prefs", { prefsUpdate }),
  setPackPinned: (packId, pinned) => call("set_pack_pinned", { packId, pinned }),
  setWindowSize: (width, height) => call("set_window_size", { width, height }),
  getOverlayPlacement: () => call("get_overlay_placement"),
  setOverlayInputFullscreen: (fullscreen) => call("set_overlay_input_fullscreen", { fullscreen }),
  setOverlayRect: (x, y, width, height, persistSize = false) =>
    call("set_overlay_rect", { x, y, width, height, persistSize }),
  getQuickActionsPlacement: () => call("get_quick_actions_placement"),
  setQuickActionsHeight: (height) => call("set_quick_actions_height", { height }),
  completeOnboarding: () => call("complete_onboarding"),
  getPlatformInfo: () => call("get_platform_info"),
  openDashboard: (route = null) => call("open_dashboard", { route }),
  openQuickActions: () => call("open_quick_actions"),
  hideQuickActions: () => call("hide_quick_actions"),
  checkForUpdate: () => call("check_for_update"),
  installUpdate: () => call("install_update"),
  quitApp: () => call("quit_app"),
};

export function currentTauriWindow() {
  return window.__TAURI__?.window?.getCurrentWindow?.() ?? null;
}

export async function listenEvent(eventName, handler) {
  const listen = tauriListen();
  if (!listen) {
    return () => {};
  }
  return listen(eventName, (event) => handler(event.payload));
}
