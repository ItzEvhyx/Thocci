/**
 * Desktop Dashboard (Light & Charcoal Dark themes).
 *
 * - Two-column layout: 246 px clipped sliding sidebar + stationary main workspace.
 * - Playback (Enable Thocci, coalesced Volume 0-100), Soundpacks (Browse, Import WAV),
 *   App (Animate Thocci, Setup guide).
 * - Soundpacks sidebar slides left in 320 ms cubic-bezier(0.55, 0.03, 0.86, 0.50)
 *   and reverses in 260 ms ease-out (0 ms under reduced motion).
 * - Preserves active pack, volume, sidebar view, and search query across theme toggles.
 */

import { api, currentTauriWindow, listenEvent } from "./bridge.js";
import { iconMarkup } from "./icons.js";
import { mountInteractiveKeyboard } from "./keyboard.js";
import { mountSidebarTrinket } from "./sidebar-trinket.js";
import { mountMascotStage } from "./stage.js";
import { mountTypingGame } from "./typing-game.js";

export function isTextEditableElement(el) {
  if (!el || typeof el.tagName !== "string") return false;
  const tag = el.tagName.toUpperCase();
  if (tag === "TEXTAREA") return true;
  if (el.isContentEditable) return true;
  if (tag === "INPUT") {
    const type = (el.type || el.getAttribute?.("type") || "text").toLowerCase();
    return !["range", "button", "submit", "reset", "checkbox", "radio"].includes(type);
  }
  return false;
}

export function isPointerBlurControl(el) {
  if (!el || typeof el.tagName !== "string") return false;
  const tag = el.tagName.toUpperCase();
  if (tag === "BUTTON") return true;
  if (tag === "INPUT") {
    const type = (el.type || el.getAttribute?.("type") || "text").toLowerCase();
    return ["range", "button", "submit", "reset", "checkbox", "radio"].includes(type);
  }
  const role = el.getAttribute?.("role");
  return role === "button" || role === "switch";
}

export function filterCatalog(catalog, query) {
  const q = (query || "").trim().toLowerCase();
  if (!q) return catalog;
  return catalog.filter(
    (pack) =>
      pack.display_name.toLowerCase().includes(q) ||
      pack.id.toLowerCase().includes(q) ||
      (pack.subtitle && pack.subtitle.toLowerCase().includes(q)) ||
      (pack.raw_name && pack.raw_name.toLowerCase().includes(q)),
  );
}

export async function mountDashboard(root, { initialPrefs, initialRoute = null }) {
  document.body.className = "mode-dashboard";

  const systemDarkQuery = window.matchMedia("(prefers-color-scheme: dark)");
  let prefs = { ...initialPrefs };
  let catalog = [];
  let status = {
    running: false,
    responsive: false,
    pid: null,
    volume: 100,
    pack: "cherry-mx",
    saved_volume: 100,
    saved_pack: "cherry-mx",
  };
  let platformInfo = null;
  let sidebarView = initialRoute === "soundpacks" ? "soundpacks" : "settings";
  let searchQuery = "";
  let banner = null; // { kind: 'error'|'warn'|'success', text, showGuide? }
  let isTogglingEngine = false;
  let isSwitchingPack = false;
  let isImporting = false;
  let volumeDragging = false;

  // Fetch Wayland layer-shell placement & initial engine state BEFORE painting
  // `.dashboard-shell` so it never flashes fullscreen (1920x1080) or wrong enabled state.
  const [initialPlacement, initialStatus, initialCatalog, initialPlatform] = await Promise.all([
    api.getOverlayPlacement().catch(() => null),
    api.getStatus().catch(() => status),
    api.listPackCatalog().catch(() => []),
    api.getPlatformInfo().catch(() => null),
  ]);
  if (initialStatus) status = initialStatus;
  if (Array.isArray(initialCatalog)) catalog = initialCatalog;
  platformInfo = initialPlatform;

  let layerPanelMode = false;
  let layerRect = { x: 380, y: 137, width: 1160, height: 806, screenWidth: 1920, screenHeight: 1080 };
  if (initialPlacement && initialPlacement.layerShell) {
    layerPanelMode = true;
    document.body.dataset.layerPanel = "true";
    layerRect = {
      x: Math.round(initialPlacement.x),
      y: Math.round(initialPlacement.y),
      width: Math.round(initialPlacement.width),
      height: Math.round(initialPlacement.height),
      screenWidth: Math.round(initialPlacement.screenWidth || window.innerWidth || 1920),
      screenHeight: Math.round(initialPlacement.screenHeight || window.innerHeight || 1080),
    };
  }
  const initialShellStyle = layerPanelMode
    ? ` style="left:${layerRect.x}px;top:${layerRect.y}px;width:${layerRect.width}px;height:${layerRect.height}px;--shell-h:${layerRect.height}px"`
    : "";
  const initialEnabledAttr = status.running ? "true" : "false";

  function resolvedTheme() {
    if (prefs.theme === "light" || prefs.theme === "dark") {
      return prefs.theme;
    }
    return systemDarkQuery.matches ? "dark" : "light";
  }

  let stage = null;

  function applyTheme() {
    const theme = resolvedTheme();
    document.documentElement.setAttribute("data-theme", theme);
    stage?.setTheme?.(theme);
    const themeBtn = root.querySelector("#themeToggleBtn");
    if (themeBtn) {
      if (theme === "dark") {
        themeBtn.innerHTML = `${iconMarkup("light-mode")}<span>Light mode</span>`;
        themeBtn.setAttribute("aria-label", "Switch to light mode");
      } else {
        themeBtn.innerHTML = `${iconMarkup("dark-mode")}<span>Dark mode</span>`;
        themeBtn.setAttribute("aria-label", "Switch to dark mode");
      }
    }
  }

  root.innerHTML = `
    <div class="dashboard-shell" data-thocci-enabled="${initialEnabledAttr}"${initialShellStyle}>
      <div class="resize-handle resize-handle--n" data-resize="North" aria-hidden="true"></div>
      <div class="resize-handle resize-handle--s" data-resize="South" aria-hidden="true"></div>
      <div class="resize-handle resize-handle--w" data-resize="West" aria-hidden="true"></div>
      <div class="resize-handle resize-handle--e" data-resize="East" aria-hidden="true"></div>
      <div class="resize-handle resize-handle--nw" data-resize="NorthWest" aria-hidden="true"></div>
      <div class="resize-handle resize-handle--ne" data-resize="NorthEast" aria-hidden="true"></div>
      <div class="resize-handle resize-handle--sw" data-resize="SouthWest" aria-hidden="true"></div>
      <div class="resize-handle resize-handle--se" data-resize="SouthEast" aria-hidden="true"></div>

      <aside
        id="sidebarContainer"
        class="sidebar-container"
        data-view="${sidebarView}"
        data-transition="none"
        aria-label="Thocci controls"
      >
        <!-- Main Settings Sidebar Pane -->
        <div id="settingsPane" class="sidebar-pane sidebar-pane--settings">
          <header class="sidebar-brand" data-tauri-drag-region>
            <div class="sidebar-brand-mark" aria-hidden="true">${iconMarkup("thocci-mark")}</div>
            <span class="sidebar-wordmark">Thocci</span>
          </header>

          <section class="sidebar-group" aria-labelledby="grpPlayback">
            <h2 id="grpPlayback" class="sidebar-group-label">Playback</h2>

            <div id="enableRow" class="sidebar-row sidebar-row--enable-master">
              <div class="sidebar-row-left">
                ${iconMarkup("power")}
                <span class="sidebar-row-title">Enable Thocci</span>
              </div>
              <button
                id="enableToggle"
                type="button"
                class="switch-btn"
                role="switch"
                aria-checked="${initialEnabledAttr}"
                aria-label="Enable Thocci"
              >
                <span class="switch-knob" aria-hidden="true"></span>
              </button>
            </div>

            <div class="sidebar-row sidebar-row--stack sidebar-row--volume">
              <div class="sidebar-row-top">
                <label for="volumeSlider" class="sidebar-row-title">Volume</label>
                <span id="volumeValue" class="sidebar-row-value">100%</span>
              </div>
              <div id="volumeControl" class="vol-control" style="--vol-ratio: 1; --vol-pct: 100%;">
                <div class="vol-fill" aria-hidden="true"></div>
                <div class="vol-dots" aria-hidden="true">
                  <span class="vol-dot" data-step="0" data-filled="true"></span>
                  <span class="vol-dot" data-step="20" data-filled="true"></span>
                  <span class="vol-dot" data-step="40" data-filled="true"></span>
                  <span class="vol-dot" data-step="60" data-filled="true"></span>
                  <span class="vol-dot" data-step="80" data-filled="true"></span>
                  <span class="vol-dot" data-step="100" data-filled="true"></span>
                </div>
                <div class="vol-thumb-keycap" aria-hidden="true">
                  ${iconMarkup("volume")}
                </div>
                <input
                  id="volumeSlider"
                  class="volume-slider"
                  type="range"
                  min="0"
                  max="100"
                  step="1"
                  value="100"
                  aria-label="Volume"
                  aria-valuetext="100 percent"
                />
              </div>
            </div>
          </section>

          <section class="sidebar-group" aria-labelledby="grpSoundpacks">
            <h2 id="grpSoundpacks" class="sidebar-group-label">Soundpacks</h2>

            <button
              id="browseSoundpacksBtn"
              type="button"
              class="sidebar-row sidebar-row--button sidebar-row--highlight"
            >
              <div class="sidebar-row-left">
                ${iconMarkup("keyboard")}
                <div class="sidebar-row-text">
                  <span class="sidebar-row-title">Browse soundpacks</span>
                  <span id="activePackSubtitle" class="sidebar-row-sub">Loading…</span>
                </div>
              </div>
              ${iconMarkup("chevron-right", "ui-icon--chevron")}
            </button>

            <button id="importWavBtn" type="button" class="sidebar-row sidebar-row--button">
              <div class="sidebar-row-left">
                ${iconMarkup("import-wav")}
                <span id="importWavLabel" class="sidebar-row-title">Import WAV</span>
              </div>
            </button>
          </section>

          <section class="sidebar-group" aria-labelledby="grpApp">
            <h2 id="grpApp" class="sidebar-group-label">App</h2>

            <div id="animateRow" class="sidebar-row">
              <div class="sidebar-row-left">
                ${iconMarkup("animate")}
                <span class="sidebar-row-title">Animate Thocci</span>
              </div>
              <button
                id="animateToggle"
                type="button"
                class="switch-btn"
                role="switch"
                aria-checked="${prefs.animate_mascot ? "true" : "false"}"
                aria-label="Animate Thocci"
              >
                <span class="switch-knob" aria-hidden="true"></span>
              </button>
            </div>

            <button id="setupGuideBtn" type="button" class="sidebar-row sidebar-row--button">
              <div class="sidebar-row-left">
                ${iconMarkup("setup-guide")}
                <span class="sidebar-row-title">Setup guide</span>
              </div>
            </button>
          </section>

          <div id="sidebarFooter" class="sidebar-footer">
            <div id="sidebarTrinket" class="sidebar-trinket" aria-hidden="true"></div>
          </div>
        </div>

        <!-- Soundpacks Browser Sidebar Pane -->
        <div id="soundpacksPane" class="sidebar-pane sidebar-pane--soundpacks">
          <header class="sp-header">
            <button
              id="spBackBtn"
              type="button"
              class="sp-back-btn"
              aria-label="Back to settings"
            >
              ${iconMarkup("chevron-right", "ui-icon--flip-x")}
            </button>
            <h2 class="sp-title">Soundpacks</h2>
          </header>

          <div class="sp-search-wrap">
            <label for="spSearchInput" class="sr-only">Search soundpacks</label>
            <span class="sp-search-icon" aria-hidden="true">${iconMarkup("search")}</span>
            <input
              id="spSearchInput"
              class="sp-search-input"
              type="search"
              placeholder="Search soundpacks"
              autocomplete="off"
              spellcheck="false"
            />
            <button
              id="spSearchClear"
              type="button"
              class="sp-search-clear"
              aria-label="Clear search"
              hidden
            >
              ×
            </button>
          </div>

          <div id="spListScroll" class="sp-list-scroll" role="region" aria-label="Soundpack catalog"></div>
        </div>
      </aside>

      <main class="workspace">
        <div class="workspace-inner">
          <header id="workspaceHeader" class="workspace-header" data-tauri-drag-region>
            <div class="workspace-heading-group" data-tauri-drag-region>
              <h1 class="workspace-title" data-tauri-drag-region>Home</h1>
              <p class="workspace-subtitle" data-tauri-drag-region>Type or click any key to preview your soundpack.</p>
            </div>
            <div class="workspace-header-right">
              <div class="workspace-header-actions">
                <button id="themeToggleBtn" type="button" class="theme-pill"></button>
                <button id="windowCloseBtn" type="button" class="window-close-btn" aria-label="Close window" title="Close">
                  <svg viewBox="0 0 12 12" width="12" height="12" aria-hidden="true">
                    <path d="M2.5 2.5L9.5 9.5M9.5 2.5L2.5 9.5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
                  </svg>
                </button>
              </div>
              <button id="challengeTriggerBtn" type="button" class="challenge-trigger-btn">
                <span>Up for a speed test? Try the 20s challenge</span>
                ${iconMarkup("chevron-right", "challenge-trigger-chevron")}
              </button>
            </div>
          </header>

          <div id="bannerSlot" role="status" aria-live="polite" hidden></div>

          <div class="stage-slot">
            <div id="stageCard" class="stage-card"></div>
            <div id="typingGameCard" class="typing-game-card" hidden></div>
          </div>
          <div id="keyboardCard" class="keyboard-card"></div>
          <div id="typedFlyLayer" class="typed-fly-layer" aria-hidden="true"></div>
        </div>
      </main>
      <div id="setupGuideModal" class="modal-backdrop" hidden></div>
      <div id="fileDialogScrim" class="file-dialog-scrim" aria-hidden="true" hidden></div>
    </div>
  `;

  applyTheme();

  const dashboardShell = root.querySelector(".dashboard-shell");
  const sidebarContainer = root.querySelector("#sidebarContainer");
  const settingsPane = root.querySelector("#settingsPane");
  const soundpacksPane = root.querySelector("#soundpacksPane");
  const enableToggle = root.querySelector("#enableToggle");
  const volumeControl = root.querySelector("#volumeControl");
  const volumeDots = Array.from(root.querySelectorAll(".vol-dot"));
  const volumeSlider = root.querySelector("#volumeSlider");
  const volumeValue = root.querySelector("#volumeValue");
  const browseSoundpacksBtn = root.querySelector("#browseSoundpacksBtn");
  const activePackSubtitle = root.querySelector("#activePackSubtitle");
  const importWavBtn = root.querySelector("#importWavBtn");
  const importWavLabel = root.querySelector("#importWavLabel");
  const animateToggle = root.querySelector("#animateToggle");
  const setupGuideBtn = root.querySelector("#setupGuideBtn");
  const sidebarTrinketEl = root.querySelector("#sidebarTrinket");
  const spBackBtn = root.querySelector("#spBackBtn");
  const spSearchInput = root.querySelector("#spSearchInput");
  const spSearchClear = root.querySelector("#spSearchClear");
  const spListScroll = root.querySelector("#spListScroll");
  const themeToggleBtn = root.querySelector("#themeToggleBtn");
  const windowCloseBtn = root.querySelector("#windowCloseBtn");
  const challengeTriggerBtn = root.querySelector("#challengeTriggerBtn");
  const workspaceHeader = root.querySelector("#workspaceHeader");
  const sidebarBrand = root.querySelector(".sidebar-brand");
  const bannerSlot = root.querySelector("#bannerSlot");
  const stageCard = root.querySelector("#stageCard");
  const typingGameCard = root.querySelector("#typingGameCard");
  const keyboardCard = root.querySelector("#keyboardCard");
  const typedFlyLayer = root.querySelector("#typedFlyLayer");
  const workspaceInner = root.querySelector(".workspace-inner");
  const setupGuideModal = root.querySelector("#setupGuideModal");
  const fileDialogScrim = root.querySelector("#fileDialogScrim");

  // Prevent pointer clicks on buttons / switches / sliders from leaving sticky DOM
  // focus (and `:focus-visible` highlights) so Space / Enter immediately return to
  // keyboard previewing instead of re-triggering the last clicked control.
  // Intentional keyboard Tab navigation and text inputs (search, window size) stay intact.
  function blurActiveElement(exceptEl = null) {
    const active = document.activeElement;
    if (
      active instanceof HTMLElement &&
      active !== document.body &&
      active !== exceptEl
    ) {
      active.blur();
    }
  }

  dashboardShell?.addEventListener(
    "mousedown",
    (ev) => {
      if (ev.button !== 0) return;
      const target = ev.target instanceof Element ? ev.target : null;
      if (!target) return;

      const editable = target.closest(
        'input:not([type="range"]):not([type="button"]):not([type="submit"]):not([type="reset"]):not([type="checkbox"]):not([type="radio"]), textarea, [contenteditable="true"]',
      );
      if (editable) return;

      const control = target.closest(
        'button, input[type="range"], input[type="checkbox"], input[type="radio"], [role="switch"]',
      );
      blurActiveElement(null);
      if (control && isPointerBlurControl(control)) {
        ev.preventDefault();
      }
    },
    true,
  );

  dashboardShell?.addEventListener("click", (ev) => {
    if (ev.detail === 0) return; // preserve keyboard-initiated Tab + Enter/Space focus
    const target = ev.target instanceof Element ? ev.target : null;
    const control = target?.closest?.(
      'button, input[type="range"], input[type="checkbox"], input[type="radio"], [role="switch"]',
    );
    if (control && document.activeElement === control) {
      control.blur();
    }
  });

  // EvhOS Settings-style launch entrance & exit animations (OutExpo pop-in + staggered groups)
  let entranceTimer = null;
  let closeAnimTimer = null;
  let isClosingWindow = false;

  function playEntranceAnimation() {
    if (!dashboardShell) return;
    isClosingWindow = false;
    if (closeAnimTimer) {
      clearTimeout(closeAnimTimer);
      closeAnimTimer = null;
    }
    dashboardShell.classList.remove("is-closing");
    const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (reducedMotion) return;
    if (entranceTimer) {
      clearTimeout(entranceTimer);
      entranceTimer = null;
    }
    dashboardShell.classList.remove("is-entering");
    void dashboardShell.offsetWidth;
    dashboardShell.classList.add("is-entering");
    entranceTimer = setTimeout(() => {
      dashboardShell.classList.remove("is-entering");
      entranceTimer = null;
    }, 520);
  }

  function closeDashboardAnimated() {
    if (isClosingWindow) return;
    const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const doClose = () => {
      isClosingWindow = false;
      void currentTauriWindow()?.close?.().catch(() => {});
    };
    if (reducedMotion || !dashboardShell) {
      doClose();
      return;
    }
    isClosingWindow = true;
    if (entranceTimer) {
      clearTimeout(entranceTimer);
      entranceTimer = null;
    }
    dashboardShell.classList.remove("is-entering");
    void dashboardShell.offsetWidth;
    dashboardShell.classList.add("is-closing");
    closeAnimTimer = setTimeout(() => {
      closeAnimTimer = null;
      doClose();
    }, 175);
  }

  // Frameless floating window drag & 8-direction edge/corner resize.
  // On Linux Wayland layer-shell compositors (Hyprland, Sway, etc.) the layer
  // surface stays fixed at (0, 0) while we move/resize `.dashboard-shell` in
  // screen-space at native 60/120fps and tighten the Wayland input mask on
  // pointerup — matching Quickshell's `Main.qml` zero-jitter drag behavior.
  // On macOS, Windows, and X11/GNOME we use standard native floating OS windows.

  function applyLayerPanelRect() {
    if (!layerPanelMode || !dashboardShell) return;
    dashboardShell.style.left = `${layerRect.x}px`;
    dashboardShell.style.top = `${layerRect.y}px`;
    dashboardShell.style.width = `${layerRect.width}px`;
    dashboardShell.style.height = `${layerRect.height}px`;
    dashboardShell.style.setProperty("--shell-h", `${layerRect.height}px`);
  }

  function applyOverlayPlacement(placement) {
    if (!placement || !placement.layerShell) return;
    layerPanelMode = true;
    document.body.dataset.layerPanel = "true";
    layerRect = {
      x: Math.round(placement.x),
      y: Math.round(placement.y),
      width: Math.round(placement.width),
      height: Math.round(placement.height),
      screenWidth: Math.round(placement.screenWidth || window.innerWidth || 1920),
      screenHeight: Math.round(placement.screenHeight || window.innerHeight || 1080),
    };
    applyLayerPanelRect();
  }

  function startLayerDrag(el, downEv) {
    const startX = layerRect.x;
    const startY = layerRect.y;
    const downX = downEv.clientX;
    const downY = downEv.clientY;
    const screenW = layerRect.screenWidth || window.innerWidth || 1920;
    const screenH = layerRect.screenHeight || window.innerHeight || 1080;

    void api.setOverlayInputFullscreen(true).catch(() => {});
    try {
      el.setPointerCapture(downEv.pointerId);
    } catch {
      // Ignore if pointer capture is unavailable
    }

    function onMove(ev) {
      const nextX = Math.max(
        -layerRect.width + 120,
        Math.min(screenW - 120, Math.round(startX + (ev.clientX - downX))),
      );
      const nextY = Math.max(
        0,
        Math.min(screenH - 60, Math.round(startY + (ev.clientY - downY))),
      );
      if (nextX === layerRect.x && nextY === layerRect.y) return;
      layerRect.x = nextX;
      layerRect.y = nextY;
      applyLayerPanelRect();
    }

    function onEnd() {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onEnd);
      window.removeEventListener("pointercancel", onEnd);
      void api
        .setOverlayRect(layerRect.x, layerRect.y, layerRect.width, layerRect.height, false)
        .catch(() => {});
    }

    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onEnd);
    window.addEventListener("pointercancel", onEnd);
  }

  function startLayerResize(handle, dir, downEv) {
    const startX = layerRect.x;
    const startY = layerRect.y;
    const startW = layerRect.width;
    const startH = layerRect.height;
    const downX = downEv.clientX;
    const downY = downEv.clientY;
    const hasW = dir.includes("West");
    const hasE = dir.includes("East");
    const hasN = dir.includes("North");
    const hasS = dir.includes("South");

    void api.setOverlayInputFullscreen(true).catch(() => {});
    try {
      handle.setPointerCapture(downEv.pointerId);
    } catch {
      // Ignore
    }

    function onMove(ev) {
      const dx = Math.round(ev.clientX - downX);
      const dy = Math.round(ev.clientY - downY);
      let nextX = startX;
      let nextY = startY;
      let nextW = startW;
      let nextH = startH;

      if (hasE) {
        nextW = Math.max(480, Math.min(3840, startW + dx));
      } else if (hasW) {
        const proposedW = Math.max(480, Math.min(3840, startW - dx));
        nextX = startX + (startW - proposedW);
        nextW = proposedW;
      }

      if (hasS) {
        nextH = Math.max(440, Math.min(2160, startH + dy));
      } else if (hasN) {
        const proposedH = Math.max(440, Math.min(2160, startH - dy));
        nextY = Math.max(0, startY + (startH - proposedH));
        nextH = startY + startH - nextY;
      }

      layerRect.x = nextX;
      layerRect.y = nextY;
      layerRect.width = nextW;
      layerRect.height = nextH;
      applyLayerPanelRect();
    }

    function onEnd() {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onEnd);
      window.removeEventListener("pointercancel", onEnd);
      void api
        .setOverlayRect(layerRect.x, layerRect.y, layerRect.width, layerRect.height, true)
        .catch(() => {});
    }

    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onEnd);
    window.addEventListener("pointercancel", onEnd);
  }

  function startWindowDrag(el, ev) {
    if (ev.button !== 0 || ev.target.closest("button, input, a")) return;
    if (layerPanelMode || platformInfo?.layer_shell_active) {
      ev.preventDefault();
      startLayerDrag(el, ev);
    } else {
      ev.preventDefault();
      void currentTauriWindow()?.startDragging?.().catch(() => {});
    }
  }

  function bindDragArea(el) {
    el?.addEventListener("mousedown", (ev) => {
      if (!(layerPanelMode || platformInfo?.layer_shell_active)) {
        startWindowDrag(el, ev);
      }
    });
    el?.addEventListener("pointerdown", (ev) => {
      if (layerPanelMode || platformInfo?.layer_shell_active) {
        startWindowDrag(el, ev);
      }
    });
  }
  bindDragArea(workspaceHeader);
  bindDragArea(sidebarBrand);

  for (const handle of root.querySelectorAll(".resize-handle")) {
    // Native OS windows (Windows WebView2, macOS WKWebView, X11) respond most
    // reliably to `startResizeDragging` initiated during `mousedown`.
    handle.addEventListener("mousedown", (ev) => {
      if (ev.button !== 0) return;
      if (layerPanelMode || platformInfo?.layer_shell_active) return;
      const dir = handle.dataset.resize;
      if (!dir) return;
      ev.preventDefault();
      ev.stopPropagation();
      void currentTauriWindow()?.startResizeDragging?.(dir).catch(() => {});
    });

    // Wayland layer-shell panels resize smoothly in screen-space via pointer capture.
    handle.addEventListener("pointerdown", (ev) => {
      if (ev.button !== 0) return;
      if (!(layerPanelMode || platformInfo?.layer_shell_active)) return;
      const dir = handle.dataset.resize;
      if (!dir) return;
      ev.preventDefault();
      startLayerResize(handle, dir, ev);
    });
  }

  windowCloseBtn?.addEventListener("click", closeDashboardAnimated);

  // ── Flying Typed-Character Stream (keycap -> center-top of Thocci's stage -> drop & fade after 0.5s) ──
  const activeTypedTokens = [];

  function layoutTypedRow() {
    if (!workspaceInner || !stageCard || activeTypedTokens.length === 0) return;
    const wsRect = workspaceInner.getBoundingClientRect();
    const stageRect = stageCard.getBoundingClientRect();
    const stageCenterX = stageRect.left - wsRect.left + stageRect.width / 2;
    const targetY = stageRect.top - wsRect.top + Math.max(10, stageRect.height * 0.075);
    const maxRowW = Math.max(120, stageRect.width - 72);

    // Never allow characters to overflow the row area: drop oldest leftmost first
    let totalW = activeTypedTokens.reduce((sum, t) => sum + t.w, 0);
    while (totalW > maxRowW && activeTypedTokens.length > 1) {
      const oldest = activeTypedTokens[0];
      totalW -= oldest.w;
      dropTypedToken(oldest, { relayout: false });
    }

    let cursorX = stageCenterX - totalW / 2;
    for (const token of activeTypedTokens) {
      token.x = cursorX;
      token.y = targetY;
      cursorX += token.w;
      if (token.launched) {
        token.el.style.transform = `translate3d(${token.x.toFixed(1)}px, ${token.y.toFixed(1)}px, 0) scale(1)`;
      }
    }
  }

  function dropTypedToken(token, { relayout = true } = {}) {
    if (!token || token.dropping) return;
    token.dropping = true;
    if (token.timerId) {
      clearTimeout(token.timerId);
      token.timerId = null;
    }
    const idx = activeTypedTokens.indexOf(token);
    if (idx !== -1) {
      activeTypedTokens.splice(idx, 1);
    }

    const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (reducedMotion) {
      token.el.remove();
    } else {
      token.el.classList.remove("is-flying");
      token.el.classList.add("is-dropping");
      token.el.style.transform = `translate3d(${token.x.toFixed(1)}px, ${(token.y + 22).toFixed(1)}px, 0) scale(0.86)`;
      token.el.style.opacity = "0";
      setTimeout(() => {
        token.el.remove();
      }, 240);
    }

    if (relayout) {
      layoutTypedRow();
    }
  }

  function spawnTypedChar({ char, action, keyRect }) {
    if (!typedFlyLayer || !workspaceInner || !stageCard) return;
    if (!setupGuideModal.hidden) return;

    if (action === "backspace") {
      const last = activeTypedTokens[activeTypedTokens.length - 1];
      if (last) dropTypedToken(last);
      return;
    }

    if (!char || char.length !== 1) return;

    const wsRect = workspaceInner.getBoundingClientRect();
    const stageRect = stageCard.getBoundingClientRect();
    if (stageRect.width < 40 || stageRect.height < 40) return;

    const isSpace = char === " ";
    const el = document.createElement("span");
    el.className = isSpace ? "typed-fly-char typed-fly-char--space" : "typed-fly-char";
    el.textContent = isSpace ? "\u00A0" : char;
    typedFlyLayer.appendChild(el);

    const measuredW = isSpace ? 13 : Math.max(8, el.getBoundingClientRect().width + 1);
    const token = {
      el,
      w: measuredW,
      x: 0,
      y: 0,
      launched: false,
      dropping: false,
      timerId: null,
    };

    activeTypedTokens.push(token);
    layoutTypedRow();

    const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (reducedMotion) {
      token.launched = true;
      el.style.transform = `translate3d(${token.x.toFixed(1)}px, ${token.y.toFixed(1)}px, 0) scale(1)`;
      el.style.opacity = "1";
    } else {
      // Originate directly on top of the pressed keycap, hidden at origin so the
      // character only appears as it glides smoothly up into Thocci's center-top row.
      const originX = keyRect
        ? keyRect.left + keyRect.width / 2 - wsRect.left - measuredW / 2
        : token.x;
      const originY = keyRect
        ? keyRect.top - wsRect.top - 4
        : token.y + 36;

      el.style.transform = `translate3d(${originX.toFixed(1)}px, ${originY.toFixed(1)}px, 0) scale(0.38)`;
      el.style.opacity = "0";
      void el.offsetWidth;

      token.launched = true;
      el.classList.add("is-flying");
      el.style.transform = `translate3d(${token.x.toFixed(1)}px, ${token.y.toFixed(1)}px, 0) scale(1)`;
      el.style.opacity = "1";
    }

    // After 2s of this key being typed, drop and fade it out cleanly
    token.timerId = setTimeout(() => {
      dropTypedToken(token);
    }, 2000);
  }

  function clearTypedStream() {
    while (activeTypedTokens.length > 0) {
      const t = activeTypedTokens.pop();
      if (t.timerId) clearTimeout(t.timerId);
      t.el.remove();
    }
  }

  // ── 20s Trivia Typing Challenge mode toggle (pop Thocci out <-> pop Challenge in) ──
  let typingGame = null;
  let stageSwapping = false;
  let stageSwapTimer = null;
  let challengeSuspended = false;
  let dashboardHidden = false;
  let disabledSuspended = !status.running;
  let guideSuspended = false;
  let trinket = null;

  function syncStageSuspension() {
    const coreSuspended = challengeSuspended || dashboardHidden || disabledSuspended;
    stage?.setSuspended?.(coreSuspended);
    trinket?.setSuspended?.(coreSuspended || guideSuspended || sidebarView !== "settings");
  }

  function openTypingChallenge() {
    if (!status.running || typingGame || stageSwapping) return;
    stageSwapping = true;
    clearTypedStream();
    challengeSuspended = true;
    syncStageSuspension();
    if (challengeTriggerBtn) challengeTriggerBtn.hidden = true;

    const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const mountGame = () => {
      stageCard.hidden = true;
      stageCard.classList.remove("stage-pop-out", "stage-pop-in");
      typingGameCard.hidden = false;
      typingGame = mountTypingGame(typingGameCard, {
        onBackToThocci: closeTypingChallenge,
      });
      if (!reducedMotion) {
        typingGameCard.classList.remove("stage-pop-out", "stage-pop-in");
        void typingGameCard.offsetWidth;
        typingGameCard.classList.add("stage-pop-in");
        stageSwapTimer = setTimeout(() => {
          typingGameCard.classList.remove("stage-pop-in");
          stageSwapping = false;
          stageSwapTimer = null;
        }, 250);
      } else {
        stageSwapping = false;
      }
    };

    if (reducedMotion) {
      mountGame();
      return;
    }

    stageCard.classList.remove("stage-pop-in");
    stageCard.classList.add("stage-pop-out");
    stageSwapTimer = setTimeout(mountGame, 175);
  }

  function closeTypingChallenge() {
    if (!typingGame || stageSwapping) return;
    stageSwapping = true;

    const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const restoreThocci = () => {
      typingGame?.destroy();
      typingGame = null;
      typingGameCard.hidden = true;
      typingGameCard.classList.remove("stage-pop-out", "stage-pop-in");
      stageCard.hidden = false;
      challengeSuspended = false;
      syncStageSuspension();
      if (challengeTriggerBtn) challengeTriggerBtn.hidden = false;
      blurActiveElement(null);
      if (!reducedMotion) {
        stageCard.classList.remove("stage-pop-out", "stage-pop-in");
        void stageCard.offsetWidth;
        stageCard.classList.add("stage-pop-in");
        stageSwapTimer = setTimeout(() => {
          stageCard.classList.remove("stage-pop-in");
          stageSwapping = false;
          stageSwapTimer = null;
        }, 250);
      } else {
        stageSwapping = false;
      }
    };

    if (reducedMotion) {
      restoreThocci();
      return;
    }

    typingGameCard.classList.remove("stage-pop-in");
    typingGameCard.classList.add("stage-pop-out");
    stageSwapTimer = setTimeout(restoreThocci, 175);
  }

  challengeTriggerBtn?.addEventListener("click", openTypingChallenge);

  // Prevent Space / Backspace / slash / quote from scrolling or triggering focused controls while sprinting
  function handleTypingGameGuard(ev) {
    if (!typingGame || typingGame.getPhase() !== "playing") return;
    if (!setupGuideModal.hidden) return;
    const tag = ev.target?.tagName;
    if (tag === "INPUT" || tag === "TEXTAREA") return;
    if (ev.code === "Space" || ev.key === " " || ev.key === "'" || ev.key === "/" || ev.key === "Backspace") {
      ev.preventDefault();
    }
  }
  window.addEventListener("keydown", handleTypingGameGuard);

  // Mount live mascot stage, sidebar keycap trinket & modern MacBook interactive keyboard
  stage = await mountMascotStage(stageCard, {
    initialTheme: resolvedTheme(),
    initialAnimated: prefs.animate_mascot,
  });

  if (sidebarTrinketEl) {
    trinket = await mountSidebarTrinket(sidebarTrinketEl, {
      initialAnimated: prefs.animate_mascot,
    });
  }

  const keyboard = await mountInteractiveKeyboard(keyboardCard, {
    onVisualActivity() {
      if (!status.running) return;
      if (!typingGame && !stageSwapping && setupGuideModal.hidden) {
        stage.reactToKeystroke();
        trinket?.reactToKeystroke();
      }
    },
    onPointerAudition(code, kind) {
      if (!status.running) return;
      void api.playPreview(code, kind).catch((err) => {
        if (kind === "down") {
          setBanner("error", `Audio preview failed: ${String(err)}`);
        }
      });
    },
    onKeyTyped(info) {
      if (!status.running) return;
      if (typingGame) {
        if (setupGuideModal.hidden) {
          typingGame.handleInput(info);
        }
        return;
      }
      spawnTypedChar(info);
    },
  });

  // ── Banner / Feedback helpers ─────────────────────────────────────────────
  let bannerTimeout = null;
  function setBanner(kind, text, { showGuide = false, autoClearMs = 0 } = {}) {
    if (bannerTimeout) {
      clearTimeout(bannerTimeout);
      bannerTimeout = null;
    }
    banner = text ? { kind, text, showGuide } : null;
    renderBanner();
    if (autoClearMs > 0 && banner) {
      bannerTimeout = setTimeout(() => {
        banner = null;
        renderBanner();
      }, autoClearMs);
    }
  }

  function renderBanner() {
    if (!banner) {
      bannerSlot.hidden = true;
      bannerSlot.innerHTML = "";
      return;
    }
    bannerSlot.hidden = false;
    bannerSlot.innerHTML = `
      <div class="banner banner--${banner.kind}">
        <span>${escapeHtml(banner.text)}</span>
        <div class="banner-actions">
          ${banner.showGuide ? `<button type="button" class="banner-btn" data-action="guide">Setup guide</button>` : ""}
          <button type="button" class="banner-btn" data-action="dismiss" aria-label="Dismiss message">Dismiss</button>
        </div>
      </div>
    `;
    bannerSlot.querySelector('[data-action="guide"]')?.addEventListener("click", openSetupGuide);
    bannerSlot.querySelector('[data-action="dismiss"]')?.addEventListener("click", () => {
      setBanner(null, "");
    });
  }

  // ── Sidebar view & animated slide transition ──────────────────────────────
  function syncPaneInert() {
    const inSoundpacks = sidebarView === "soundpacks";
    settingsPane.inert = inSoundpacks;
    soundpacksPane.inert = !inSoundpacks;
    settingsPane.setAttribute("aria-hidden", inSoundpacks ? "true" : "false");
    soundpacksPane.setAttribute("aria-hidden", inSoundpacks ? "false" : "true");
    syncStageSuspension();
  }

  function setSidebarView(nextView, { animate = true, manageFocus = true } = {}) {
    if (sidebarView === nextView) {
      syncPaneInert();
      if (manageFocus && nextView === "soundpacks") {
        spSearchInput.focus({ preventScroll: true });
      }
      return;
    }

    const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (!animate || reducedMotion) {
      sidebarContainer.setAttribute("data-transition", "none");
    } else {
      sidebarContainer.setAttribute(
        "data-transition",
        nextView === "soundpacks" ? "opening" : "closing",
      );
    }

    sidebarView = nextView;
    sidebarContainer.scrollLeft = 0;
    sidebarContainer.setAttribute("data-view", sidebarView);
    syncPaneInert();

    if (manageFocus) {
      if (sidebarView === "soundpacks") {
        spSearchInput.focus({ preventScroll: true });
        scrollActivePackIntoView();
      } else {
        browseSoundpacksBtn.focus({ preventScroll: true });
      }
    }
  }

  syncPaneInert();

  // ── Status & Catalog rendering ────────────────────────────────────────────
  function findPackById(id) {
    return catalog.find((p) => p.id === id) || null;
  }

  function updateVolumeUi(percent) {
    const clamped = Math.max(0, Math.min(100, Math.round(percent)));
    const ratio = clamped / 100;
    volumeSlider.value = String(clamped);
    volumeSlider.setAttribute("aria-valuetext", `${clamped} percent`);
    if (volumeControl) {
      volumeControl.style.setProperty("--vol-pct", `${clamped}%`);
      volumeControl.style.setProperty("--vol-ratio", ratio.toFixed(4));
    }
    for (const dot of volumeDots) {
      const step = Number(dot.dataset.step);
      dot.setAttribute("data-filled", clamped >= step ? "true" : "false");
    }
    volumeValue.textContent = `${clamped}%`;
  }

  function renderStatus() {
    const enabled = Boolean(status.running);
    enableToggle.setAttribute("aria-checked", enabled ? "true" : "false");
    if (dashboardShell) {
      dashboardShell.dataset.thocciEnabled = enabled ? "true" : "false";
    }
    disabledSuspended = !enabled;
    syncStageSuspension();
    keyboard.setEnabled(enabled);

    volumeSlider.disabled = !enabled;
    browseSoundpacksBtn.disabled = !enabled;
    importWavBtn.disabled = !enabled || isImporting;
    animateToggle.disabled = !enabled;
    themeToggleBtn.disabled = !enabled;
    if (challengeTriggerBtn) challengeTriggerBtn.disabled = !enabled;
    spSearchInput.disabled = !enabled;

    if (!enabled) {
      clearTypedStream();
      if (typingGame) {
        closeTypingChallenge();
      }
      if (sidebarView === "soundpacks") {
        setSidebarView("settings", { animate: true, manageFocus: false });
      }
    }

    if (!volumeDragging) {
      updateVolumeUi(status.volume);
    }

    const activeEntry = findPackById(status.pack);
    activePackSubtitle.textContent = activeEntry ? activeEntry.display_name : status.pack;

    if (status.running && status.responsive === false) {
      setBanner(
        "warn",
        `Background engine (pid ${status.pid ?? "?"}) is not responding to status queries yet.`,
        { showGuide: true },
      );
    }

    renderCatalogList();
  }

  function renderCatalogList() {
    const filtered = filterCatalog(catalog, searchQuery);
    const builtins = filtered.filter((p) => p.origin === "builtin");
    const imported = filtered.filter((p) => p.origin === "imported");
    const totalBuiltins = catalog.filter((p) => p.origin === "builtin").length;
    const totalImported = catalog.filter((p) => p.origin === "imported").length;
    const isSearching = searchQuery.trim().length > 0;

    if (filtered.length === 0) {
      spListScroll.innerHTML = `
        <div class="sp-empty">
          <p class="sp-empty-text">No soundpacks match “${escapeHtml(searchQuery.trim())}”.</p>
          <button id="spEmptyClearBtn" type="button" class="sp-empty-clear">Clear search</button>
        </div>
      `;
      spListScroll.querySelector("#spEmptyClearBtn")?.addEventListener("click", () => {
        searchQuery = "";
        spSearchInput.value = "";
        spSearchClear.hidden = true;
        renderCatalogList();
        spSearchInput.focus();
      });
      return;
    }

    const sectionsHtml = [];
    if (builtins.length > 0) {
      const countLabel = isSearching ? `${builtins.length} / ${totalBuiltins}` : String(totalBuiltins);
      sectionsHtml.push(renderPackSection("Built-in", countLabel, builtins));
    }
    if (imported.length > 0) {
      const countLabel = isSearching ? `${imported.length} / ${totalImported}` : String(totalImported);
      sectionsHtml.push(renderPackSection("Imported", countLabel, imported));
    }

    spListScroll.innerHTML = sectionsHtml.join("");

    for (const btn of spListScroll.querySelectorAll(".sp-item")) {
      btn.addEventListener("click", () => {
        const packId = btn.dataset.packId;
        if (packId) void selectPack(packId);
      });
    }
  }

  function renderPackSection(label, countText, items) {
    return `
      <section class="sp-section" aria-label="${escapeHtml(label)} soundpacks">
        <div class="sp-section-header">
          <span>${escapeHtml(label)}</span>
          <span>${escapeHtml(countText)}</span>
        </div>
        ${items
          .map((pack) => {
            const active = pack.id === status.pack;
            return `
              <button
                type="button"
                class="sp-item"
                data-pack-id="${escapeHtml(pack.id)}"
                aria-current="${active ? "true" : "false"}"
                ${isSwitchingPack ? "disabled" : ""}
              >
                <span class="sp-item-badge" aria-hidden="true">${iconMarkup("keyboard")}</span>
                <span class="sp-item-copy">
                  <span class="sp-item-name">${escapeHtml(pack.display_name)}</span>
                  <span class="sp-item-sub">${escapeHtml(pack.subtitle)}</span>
                </span>
                ${
                  active
                    ? `<span class="sp-item-check" aria-label="Active soundpack">${iconMarkup("check")}</span>`
                    : ""
                }
              </button>
            `;
          })
          .join("")}
      </section>
    `;
  }

  function scrollActivePackIntoView() {
    const activeBtn = spListScroll.querySelector('.sp-item[aria-current="true"]');
    if (!activeBtn) return;
    // Scroll vertically within spListScroll only — never scroll outer sidebar panes horizontally.
    const top = activeBtn.offsetTop;
    const bottom = top + activeBtn.offsetHeight;
    if (top < spListScroll.scrollTop) {
      spListScroll.scrollTop = Math.max(0, top - 8);
    } else if (bottom > spListScroll.scrollTop + spListScroll.clientHeight) {
      spListScroll.scrollTop = bottom - spListScroll.clientHeight + 8;
    }
  }

  // ── Engine Mutations (serialized & reconciled) ────────────────────────────
  async function reconcileAll() {
    try {
      const [freshCatalog, freshStatus] = await Promise.all([
        api.listPackCatalog(),
        api.getStatus(),
      ]);
      catalog = freshCatalog;
      status = freshStatus;
      renderStatus();
    } catch (err) {
      setBanner("error", `Could not read engine state: ${String(err)}`);
    }
  }

  async function selectPack(packId) {
    if (isSwitchingPack) return;
    isSwitchingPack = true;
    renderCatalogList();

    try {
      const confirmed = await api.setPack(packId);
      status = confirmed;
      renderStatus();
      // Audition one gentle key sample from the confirmed pack
      void api.playPreview("KeyA", "down").catch(() => {});
    } catch (err) {
      const fresh = await api.getStatus().catch(() => status);
      status = fresh;
      renderStatus();
      setBanner("error", String(err));
    } finally {
      isSwitchingPack = false;
      renderCatalogList();
    }
  }

  enableToggle.addEventListener("click", async () => {
    if (isTogglingEngine) return;
    isTogglingEngine = true;
    enableToggle.disabled = true;
    setBanner(null, "");

    try {
      const confirmed = await api.toggleDaemon();
      status = confirmed;
      renderStatus();
    } catch (err) {
      const raw = String(err);
      const needsPermission = /permission|input monitoring|input.*group|event tap|\/dev\/input/i.test(raw);
      const fresh = await api.getStatus().catch(() => status);
      status = fresh;
      renderStatus();
      setBanner("error", raw, { showGuide: needsPermission });
    } finally {
      isTogglingEngine = false;
      enableToggle.disabled = false;
    }
  });

  // Coalesced volume updates during drag + guaranteed final flush on change/release
  let volumeDebounceTimer = null;
  let volumeInFlight = false;
  let queuedVolume = null;

  async function flushVolume(level) {
    if (volumeInFlight) {
      queuedVolume = level;
      return;
    }
    volumeInFlight = true;
    queuedVolume = null;
    try {
      const confirmed = await api.setVolume(level);
      status = confirmed;
      if (!volumeDragging) {
        updateVolumeUi(status.volume);
      }
    } catch (err) {
      setBanner("warn", String(err), { showGuide: true });
      const fresh = await api.getStatus().catch(() => status);
      status = fresh;
      if (!volumeDragging) updateVolumeUi(status.volume);
    } finally {
      volumeInFlight = false;
      if (queuedVolume !== null) {
        const next = queuedVolume;
        queuedVolume = null;
        void flushVolume(next);
      }
    }
  }

  // Magnetic 20-step dot snapping (0/20/40/60/80/100) + smooth free hold-and-drag
  const VOLUME_STOPS = [0, 20, 40, 60, 80, 100];
  let volPointerDownX = 0;
  let volPointerMoved = false;

  function applyVolumeMagnet(rawVal, threshold) {
    let best = rawVal;
    let bestDist = threshold + 1;
    for (const stop of VOLUME_STOPS) {
      const dist = Math.abs(rawVal - stop);
      if (dist <= threshold && dist < bestDist) {
        best = stop;
        bestDist = dist;
      }
    }
    return best;
  }

  function valueFromVolumePointer(clientX) {
    const rect = volumeControl.getBoundingClientRect();
    const thumbW = 38;
    const travel = Math.max(1, rect.width - thumbW);
    const ratio = Math.max(0, Math.min(1, (clientX - rect.left - thumbW / 2) / travel));
    return Math.round(ratio * 100);
  }

  function scheduleVolumeFlush(val) {
    if (volumeDebounceTimer) clearTimeout(volumeDebounceTimer);
    volumeDebounceTimer = setTimeout(() => {
      volumeDebounceTimer = null;
      void flushVolume(val);
    }, 85);
  }

  volumeSlider.addEventListener("pointerdown", (ev) => {
    if (!status.running) return;
    if (ev.pointerType === "mouse" && ev.button !== 0) return;
    ev.preventDefault();
    volumeDragging = true;
    volPointerDownX = ev.clientX;
    volPointerMoved = false;
    volumeControl?.setAttribute("data-dragging", "true");
    volumeControl?.setAttribute("data-snapping", "true");
    try {
      volumeSlider.setPointerCapture(ev.pointerId);
    } catch {
      // Ignore
    }
    // Immediate click/tap near any 20-step dot magnets cleanly onto it
    const raw = valueFromVolumePointer(ev.clientX);
    const val = applyVolumeMagnet(raw, 7);
    updateVolumeUi(val);
    scheduleVolumeFlush(val);
  });

  volumeSlider.addEventListener("pointermove", (ev) => {
    if (!volumeDragging) return;
    if (!volPointerMoved && Math.abs(ev.clientX - volPointerDownX) >= 4) {
      volPointerMoved = true;
      volumeControl?.setAttribute("data-snapping", "false");
    }
    if (!volPointerMoved) return;
    // Free continuous drag with a subtle ±2 magnetic detent over the 20-step dots
    const raw = valueFromVolumePointer(ev.clientX);
    const val = applyVolumeMagnet(raw, 2);
    if (val !== Number(volumeSlider.value)) {
      updateVolumeUi(val);
      scheduleVolumeFlush(val);
    }
  });

  const endVolumeDrag = (ev) => {
    if (!volumeDragging) return;
    volumeDragging = false;
    volumeControl?.setAttribute("data-dragging", "false");
    volumeControl?.setAttribute("data-snapping", "false");
    if (ev && typeof ev.pointerId === "number") {
      try {
        volumeSlider.releasePointerCapture(ev.pointerId);
      } catch {
        // Ignore
      }
    }
    const val = Number(volumeSlider.value);
    if (volumeDebounceTimer) {
      clearTimeout(volumeDebounceTimer);
      volumeDebounceTimer = null;
    }
    if (document.activeElement === volumeSlider) {
      volumeSlider.blur();
    }
    void flushVolume(val);
  };
  volumeSlider.addEventListener("pointerup", endVolumeDrag);
  volumeSlider.addEventListener("pointercancel", endVolumeDrag);

  // Keyboard accessibility (ArrowLeft/Right, Home/End, PageUp/PageDown)
  volumeSlider.addEventListener("input", () => {
    if (volumeDragging) return;
    const val = Number(volumeSlider.value);
    updateVolumeUi(val);
    scheduleVolumeFlush(val);
  });

  volumeSlider.addEventListener("change", () => {
    if (volumeDragging) return;
    const val = Number(volumeSlider.value);
    updateVolumeUi(val);
    if (volumeDebounceTimer) {
      clearTimeout(volumeDebounceTimer);
      volumeDebounceTimer = null;
    }
    void flushVolume(val);
  });

  // Browse soundpacks & Back navigation
  browseSoundpacksBtn.addEventListener("click", (ev) => {
    if (!status.running) return;
    setSidebarView("soundpacks", { animate: true, manageFocus: ev.detail === 0 });
  });

  spBackBtn.addEventListener("click", (ev) => {
    setSidebarView("settings", { animate: true, manageFocus: ev.detail === 0 });
  });

  spSearchInput.addEventListener("keydown", (ev) => {
    if (ev.key === "Enter") {
      ev.preventDefault();
      spSearchInput.blur();
    }
  });

  spSearchInput.addEventListener("input", () => {
    searchQuery = spSearchInput.value;
    spSearchClear.hidden = searchQuery.length === 0;
    renderCatalogList();
  });

  spSearchClear.addEventListener("click", () => {
    searchQuery = "";
    spSearchInput.value = "";
    spSearchClear.hidden = true;
    renderCatalogList();
    spSearchInput.focus();
  });

  // Import WAV
  importWavBtn.addEventListener("click", async () => {
    if (!status.running || isImporting) return;
    isImporting = true;
    importWavBtn.disabled = true;
    importWavLabel.textContent = "Importing…";

    try {
      const result = await api.pickAndImportWav();
      if (result) {
        catalog = result.catalog;
        status = result.status;
        renderStatus();
        setSidebarView("soundpacks", { animate: true, manageFocus: false });
        // Automatically select the newly imported pack (preserving old pack if switch fails)
        await selectPack(result.imported.id);
        setBanner(
          "success",
          `Imported “${result.imported.display_name}” and added it to your library.`,
          { autoClearMs: 5000 },
        );
      }
    } catch (err) {
      setBanner("error", `WAV import failed: ${String(err)}`);
    } finally {
      isImporting = false;
      importWavBtn.disabled = false;
      importWavLabel.textContent = "Import WAV";
    }
  });

  // Animate Thocci toggle (persisted separately in ui-prefs.json)
  animateToggle.addEventListener("click", async () => {
    if (!status.running) return;
    const nextVal = !prefs.animate_mascot;
    prefs = { ...prefs, animate_mascot: nextVal };
    animateToggle.setAttribute("aria-checked", nextVal ? "true" : "false");
    stage.setAnimated(nextVal);
    trinket?.setAnimated(nextVal);
    try {
      prefs = await api.saveUiPrefs(prefs);
    } catch (err) {
      setBanner("error", `Could not save animation preference: ${String(err)}`);
    }
  });

  // GPU-composited circular bottom-left theme reveal (slow start -> steadily accelerating).
  // Uses the native View Transitions API scoped strictly to `.dashboard-shell` so only
  // two flat GPU textures are composited (zero DOM cloning, zero live SVG re-rasterization),
  // with a lightweight 1-div `transform: scale()` fallback if View Transitions are unavailable.
  let activeThemeAnim = null;
  let activeThemeWave = null;
  let themeTransitionSeq = 0;

  function cleanupThemeTransition() {
    if (activeThemeAnim) {
      try {
        activeThemeAnim.cancel();
      } catch {
        // Ignore already-finished animation
      }
      activeThemeAnim = null;
    }
    if (activeThemeWave) {
      activeThemeWave.remove();
      activeThemeWave = null;
    }
    dashboardShell?.classList.remove("is-theme-vt");
  }

  function runFallbackThemeWave(nextTheme, onDone) {
    const rect = dashboardShell.getBoundingClientRect();
    const maxRadius = Math.ceil(Math.hypot(rect.width, rect.height)) + 20;
    const targetScale = maxRadius / 50; // wave base circle has 50px radius (100x100)

    const wave = document.createElement("div");
    wave.className = "theme-wave-fallback";
    wave.style.background = nextTheme === "dark" ? "#111112" : "#FDFDFF";
    dashboardShell.appendChild(wave);
    activeThemeWave = wave;

    const expandAnim = wave.animate(
      [
        { transform: "translate3d(0, 0, 0) scale(0.01)" },
        { transform: `translate3d(0, 0, 0) scale(${targetScale.toFixed(3)})` },
      ],
      {
        duration: 320,
        easing: "cubic-bezier(0.45, 0, 0.9, 0.55)",
        fill: "forwards",
      },
    );
    activeThemeAnim = expandAnim;
    expandAnim.onfinish = () => {
      if (activeThemeAnim !== expandAnim) return;
      prefs = { ...prefs, theme: nextTheme };
      applyTheme();
      const fadeAnim = wave.animate([{ opacity: 1 }, { opacity: 0 }], {
        duration: 160,
        easing: "ease-out",
        fill: "forwards",
      });
      activeThemeAnim = fadeAnim;
      fadeAnim.onfinish = () => {
        if (activeThemeAnim !== fadeAnim) return;
        cleanupThemeTransition();
        onDone?.();
      };
    };
  }

  function transitionTheme(nextTheme, onDone = null) {
    const prevTheme = resolvedTheme();
    const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const seq = ++themeTransitionSeq;
    cleanupThemeTransition();

    if (prevTheme === nextTheme || reducedMotion || !dashboardShell) {
      prefs = { ...prefs, theme: nextTheme };
      applyTheme();
      onDone?.();
      return;
    }

    if (typeof document.startViewTransition === "function") {
      const rect = dashboardShell.getBoundingClientRect();
      const maxRadius = Math.ceil(Math.hypot(rect.width, rect.height)) + 16;
      dashboardShell.classList.add("is-theme-vt");

      try {
        const vt = document.startViewTransition(() => {
          prefs = { ...prefs, theme: nextTheme };
          applyTheme();
        });

        vt.ready
          .then(() => {
            if (seq !== themeTransitionSeq) return;
            const anim = document.documentElement.animate(
              {
                clipPath: [
                  "circle(0px at 0% 100%)",
                  `circle(${maxRadius}px at 0% 100%)`,
                ],
              },
              {
                duration: 460,
                easing: "cubic-bezier(0.45, 0, 0.9, 0.55)",
                pseudoElement: "::view-transition-new(thocci-window)",
              },
            );
            activeThemeAnim = anim;
          })
          .catch(() => {
            // View transition aborted or superseded
          });

        vt.finished.finally(() => {
          if (seq === themeTransitionSeq) {
            cleanupThemeTransition();
          }
          onDone?.();
        });
        return;
      } catch {
        dashboardShell.classList.remove("is-theme-vt");
      }
    }

    runFallbackThemeWave(nextTheme, onDone);
  }

  themeToggleBtn.addEventListener("click", () => {
    if (!status.running) return;
    const nextTheme = resolvedTheme() === "dark" ? "light" : "dark";
    transitionTheme(nextTheme, async () => {
      try {
        prefs = await api.saveUiPrefs(prefs);
      } catch (err) {
        setBanner("error", `Could not save theme preference: ${String(err)}`);
      }
    });
  });

  systemDarkQuery.addEventListener("change", () => {
    if (prefs.theme === null) applyTheme();
  });

  // ── Setup Guide Modal ─────────────────────────────────────────────────────
  let setupGuideOpenedByKeyboard = false;
  let modalAnimTimer = null;
  let isClosingModal = false;

  function openSetupGuide({ focusClose = false, animate = true } = {}) {
    setupGuideOpenedByKeyboard = Boolean(focusClose);
    isClosingModal = false;
    guideSuspended = true;
    syncStageSuspension();
    if (modalAnimTimer) {
      clearTimeout(modalAnimTimer);
      modalAnimTimer = null;
    }
    const info = platformInfo || {
      os: "desktop",
      arch: "",
      session_type: "unknown",
      tray_available: false,
      input_permission_ready: null,
      daemon_exe: "thocci",
      permission_title: "Keyboard access",
      permission_steps: ["Grant keyboard monitoring access for background sounds."],
    };

    const permBadge =
      info.input_permission_ready === true
        ? "Ready"
        : info.input_permission_ready === false
          ? "Permission needed"
          : "Check System Settings";

    setupGuideModal.classList.remove("is-closing", "is-entering");
    setupGuideModal.hidden = false;
    setupGuideModal.innerHTML = `
      <div class="modal-card" role="dialog" aria-modal="true" aria-labelledby="setupGuideHeading">
        <header class="modal-header">
          <h2 id="setupGuideHeading" class="modal-title">Setup guide</h2>
          <button id="guideCloseTop" type="button" class="modal-close" aria-label="Close setup guide">×</button>
        </header>
        <div class="modal-body">
          <section class="guide-section">
            <h3>System status</h3>
            <div class="guide-diag-grid">
              <div class="guide-diag-item">
                <span class="guide-diag-label">Platform</span>
                <span class="guide-diag-value">${escapeHtml(info.os)} (${escapeHtml(info.arch)})</span>
              </div>
              <div class="guide-diag-item">
                <span class="guide-diag-label">Session / Tray</span>
                <span class="guide-diag-value">${escapeHtml(info.session_type || "standard")} · ${info.tray_available ? "Tray active" : "Dashboard fallback"}</span>
              </div>
              <div class="guide-diag-item">
                <span class="guide-diag-label">Input access</span>
                <span class="guide-diag-value">${escapeHtml(permBadge)}</span>
              </div>
              <div class="guide-diag-item">
                <span class="guide-diag-label">Engine</span>
                <span class="guide-diag-value">${status.running ? `Running (pid ${status.pid ?? "?"})` : "Stopped"}</span>
              </div>
            </div>
          </section>

          <section class="guide-section">
            <h3>1. ${escapeHtml(info.permission_title)}</h3>
            <ol>
              ${info.permission_steps.map((step) => `<li>${escapeHtml(step)}</li>`).join("")}
            </ol>
            <p style="margin-top:8px">Capture executable: <code class="guide-code">${escapeHtml(info.daemon_exe)}</code></p>
          </section>

          <section class="guide-section">
            <h3>2. Soundpacks &amp; local WAV import</h3>
            <ul>
              <li>Click any key on the keyboard to audition the active soundpack even while background playback is off.</li>
              <li>Use <strong>Import WAV</strong> to add any local <code class="guide-code">.wav</code> clip (up to 10 s / 10 MB). Imported packs stay on your device and work in both the GUI and <code class="guide-code">thocci</code> CLI.</li>
              <li>Changes stay synchronized with the CLI: try <code class="guide-code">thocci status</code>, <code class="guide-code">thocci packs</code>, or <code class="guide-code">thocci volume 60</code>.</li>
            </ul>
          </section>

          <section class="guide-section">
            <h3>Default overlay window size</h3>
            <p>Set Thocci’s default floating size (or drag any window edge/corner to resize freely).</p>
            <div class="guide-size-row">
              <label for="guideWinW" class="sr-only">Default width</label>
              <input id="guideWinW" class="guide-size-input" type="number" min="480" max="3840" value="${escapeHtml(prefs.window_width ?? 1160)}" />
              <span style="color:var(--text-muted);font-size:12px">×</span>
              <label for="guideWinH" class="sr-only">Default height</label>
              <input id="guideWinH" class="guide-size-input" type="number" min="440" max="2160" value="${escapeHtml(prefs.window_height ?? 806)}" />
              <button id="guideSizeApply" type="button" class="btn-secondary" style="height:32px;padding:0 12px;font-size:12px">Apply</button>
              <button id="guideSizeReset" type="button" class="btn-secondary" style="height:32px;padding:0 10px;font-size:12px">Reset (1160×806)</button>
            </div>
          </section>

          <section class="guide-section">
            <h3>3. Recovery from common issues</h3>
            <ul>
              <li><strong>No sound while typing globally:</strong> Confirm <em>Enable Thocci</em> is on, Volume is above 0%, and keyboard input permission is granted for your OS.</li>
              <li><strong>Unresponsive engine:</strong> Toggle <em>Enable Thocci</em> off and back on, or run <code class="guide-code">thocci off &amp;&amp; thocci on</code>.</li>
              <li><strong>Audio device switched:</strong> Thocci automatically detects output device changes within two seconds.</li>
            </ul>
          </section>
        </div>
        <footer class="modal-footer">
          <button id="guideQuitBtn" type="button" class="btn-danger-ghost">Quit Thocci</button>
          <div style="display:flex;gap:8px;flex-wrap:wrap;justify-content:flex-end">
            ${!info.tray_available ? `<button id="guideQuickActionsBtn" type="button" class="btn-secondary">Open Quick Actions</button>` : ""}
            <button id="guideRefreshBtn" type="button" class="btn-secondary">Refresh status</button>
            <button id="guideDoneBtn" type="button" class="btn-primary">Done</button>
          </div>
        </footer>
      </div>
    `;

    const closeBtn = setupGuideModal.querySelector("#guideCloseTop");
    const doneBtn = setupGuideModal.querySelector("#guideDoneBtn");
    const refreshBtn = setupGuideModal.querySelector("#guideRefreshBtn");
    const quickActionsBtn = setupGuideModal.querySelector("#guideQuickActionsBtn");
    const quitBtn = setupGuideModal.querySelector("#guideQuitBtn");
    const winWInput = setupGuideModal.querySelector("#guideWinW");
    const winHInput = setupGuideModal.querySelector("#guideWinH");
    const sizeApplyBtn = setupGuideModal.querySelector("#guideSizeApply");
    const sizeResetBtn = setupGuideModal.querySelector("#guideSizeReset");

    sizeApplyBtn?.addEventListener("click", async () => {
      const w = Math.max(480, Math.min(3840, Math.round(Number(winWInput.value) || 1160)));
      const h = Math.max(440, Math.min(2160, Math.round(Number(winHInput.value) || 806)));
      try {
        prefs = await api.setWindowSize(w, h);
        winWInput.value = String(prefs.window_width ?? 1160);
        winHInput.value = String(prefs.window_height ?? 806);
      } catch (err) {
        setBanner("error", `Could not update window size: ${String(err)}`);
      }
    });

    sizeResetBtn?.addEventListener("click", async () => {
      try {
        prefs = await api.setWindowSize(null, null);
        winWInput.value = "1160";
        winHInput.value = "806";
      } catch (err) {
        setBanner("error", `Could not reset window size: ${String(err)}`);
      }
    });

    closeBtn.addEventListener("click", (ev) => closeSetupGuide({ restoreFocus: ev.detail === 0 }));
    doneBtn.addEventListener("click", (ev) => closeSetupGuide({ restoreFocus: ev.detail === 0 }));
    refreshBtn.addEventListener("click", async () => {
      platformInfo = await api.getPlatformInfo().catch(() => platformInfo);
      await reconcileAll();
      openSetupGuide({ focusClose: false, animate: false });
    });
    quickActionsBtn?.addEventListener("click", () => {
      closeSetupGuide({ restoreFocus: false });
      void api.openQuickActions().catch(() => {});
    });
    quitBtn.addEventListener("click", () => {
      void api.quitApp().catch(() => {});
    });

    const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (animate && !reducedMotion) {
      void setupGuideModal.offsetWidth;
      setupGuideModal.classList.add("is-entering");
      modalAnimTimer = setTimeout(() => {
        setupGuideModal.classList.remove("is-entering");
        modalAnimTimer = null;
      }, 460);
    }

    if (focusClose) {
      closeBtn.focus({ preventScroll: true });
    } else {
      blurActiveElement(null);
    }
  }

  function closeSetupGuide({ restoreFocus = false } = {}) {
    if (setupGuideModal.hidden || isClosingModal) return;
    if (modalAnimTimer) {
      clearTimeout(modalAnimTimer);
      modalAnimTimer = null;
    }

    const finalizeClose = () => {
      isClosingModal = false;
      setupGuideModal.classList.remove("is-entering", "is-closing");
      setupGuideModal.hidden = true;
      setupGuideModal.innerHTML = "";
      guideSuspended = false;
      syncStageSuspension();
      if (restoreFocus) {
        setupGuideBtn.focus({ preventScroll: true });
      } else {
        blurActiveElement(null);
      }
    };

    const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (reducedMotion) {
      finalizeClose();
      return;
    }

    isClosingModal = true;
    setupGuideModal.classList.remove("is-entering");
    void setupGuideModal.offsetWidth;
    setupGuideModal.classList.add("is-closing");
    modalAnimTimer = setTimeout(() => {
      modalAnimTimer = null;
      finalizeClose();
    }, 175);
  }

  setupGuideBtn.addEventListener("click", (ev) => {
    openSetupGuide({ focusClose: ev.detail === 0 });
  });
  setupGuideModal.addEventListener("click", (ev) => {
    if (ev.target === setupGuideModal) closeSetupGuide({ restoreFocus: false });
  });

  // Global keyboard shortcuts (Escape closes modal or Soundpacks sidebar)
  function handleDashboardKeyDown(ev) {
    if (ev.key === "Escape") {
      if (!setupGuideModal.hidden) {
        ev.preventDefault();
        closeSetupGuide({ restoreFocus: setupGuideOpenedByKeyboard });
        return;
      }
      if (document.activeElement === spSearchInput) {
        ev.preventDefault();
        spSearchInput.blur();
        return;
      }
      if (sidebarView === "soundpacks") {
        ev.preventDefault();
        setSidebarView("settings", { animate: true, manageFocus: false });
      }
    }
  }
  window.addEventListener("keydown", handleDashboardKeyDown);

  // Tauri live sync events & focus reconciliation
  const unlistenState = await listenEvent("thocci://state-changed", (st) => {
    if (st) {
      status = st;
      renderStatus();
    }
  });

  const unlistenCatalog = await listenEvent("thocci://catalog-changed", (nextCat) => {
    if (Array.isArray(nextCat)) {
      catalog = nextCat;
      renderStatus();
    }
  });

  const unlistenNavigate = await listenEvent("thocci://navigate", (payload) => {
    if (payload?.view === "soundpacks") {
      setSidebarView("soundpacks", { animate: true, manageFocus: true });
    } else if (payload?.view === "settings") {
      setSidebarView("settings", { animate: true, manageFocus: true });
    }
    void reconcileAll();
  });

  const unlistenPrefs = await listenEvent("thocci://prefs-changed", (nextPrefs) => {
    if (nextPrefs) {
      prefs = nextPrefs;
      applyTheme();
      animateToggle.setAttribute("aria-checked", prefs.animate_mascot ? "true" : "false");
      stage.setAnimated(prefs.animate_mascot);
      trinket?.setAnimated(prefs.animate_mascot);
    }
  });

  const unlistenOverlay = await listenEvent("thocci://overlay-placement", (placement) => {
    applyOverlayPlacement(placement);
  });

  const unlistenShown = await listenEvent("thocci://dashboard-shown", () => {
    dashboardHidden = false;
    syncStageSuspension();
    playEntranceAnimation();
    void reconcileAll();
  });

  const unlistenHidden = await listenEvent("thocci://dashboard-hidden", () => {
    dashboardHidden = true;
    syncStageSuspension();
  });

  const unlistenFileDialog = await listenEvent("thocci://file-dialog-state", (isOpen) => {
    if (fileDialogScrim) {
      fileDialogScrim.hidden = !isOpen;
    }
  });

  window.addEventListener("focus", () => {
    void reconcileAll();
  });

  // Apply already-loaded initial status/platform & trigger clean pop-in entrance
  renderStatus();
  playEntranceAnimation();

  return {
    destroy() {
      if (entranceTimer) clearTimeout(entranceTimer);
      if (closeAnimTimer) clearTimeout(closeAnimTimer);
      if (modalAnimTimer) clearTimeout(modalAnimTimer);
      if (stageSwapTimer) clearTimeout(stageSwapTimer);
      typingGame?.destroy();
      clearTypedStream();
      cleanupThemeTransition();
      window.removeEventListener("keydown", handleTypingGameGuard);
      window.removeEventListener("keydown", handleDashboardKeyDown);
      keyboard.destroy();
      stage.destroy();
      trinket?.destroy();
      unlistenOverlay();
      unlistenShown();
      unlistenHidden();
      unlistenFileDialog();
      unlistenState();
      unlistenCatalog();
      unlistenNavigate();
      unlistenPrefs();
    },
  };
}

function escapeHtml(text) {
  return String(text ?? "")
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}
