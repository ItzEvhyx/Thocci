/**
 * Quick Actions tray / menu-bar popover (288 x 320 collapsed, up to 288 x 588 expanded).
 *
 * - Wordmark "Thocci", layered mascot behind black deck with bounded
 *   smooth pointer-follow eyes (#thocci-eyes).
 * - Left keyboard keycap toggles engine playback, updates status after
 *   confirmation ("Thocci is on" / "Thocci is off"), keeps popover open.
 * - Right gear keycap ("Settings") opens/focuses dashboard and dismisses popover.
 * - Eight individual "S W I T C H E S" keycaps share one accessible
 *   "Browse soundpacks" control, show individual cap pressed feedback, play
 *   matching letter audition (only when enabled), and route directly to the Soundpacks sidebar.
 * - Wide "Pinned" keycap below SWITCHES expands/collapses the user's pinned
 *   soundpacks (up to 4 whole cards + slice of 5th, scrolling internally to all 10).
 */

import { api, listenEvent } from "./bridge.js";
import { iconMarkup, loadTextAsset } from "./icons.js";
import { renderPackCard, replaceCards, resolvePinnedPacks } from "./pack-cards.js";

export const QA_COLLAPSED_HEIGHT = 320;
export const QA_MAX_EXPANDED_HEIGHT = 588;
const QA_EMPTY_LIST_HEIGHT = 60;
const QA_CARD_STRIDE = 59; // 52px card + 7px gap
const QA_CARD_GAP = 7;
const QA_FOUR_PLUS_SLICE_HEIGHT = 255; // 4 full 52px cards + 3 gaps + 7px gap + 19px of 5th
const QA_PANEL_EXTRA = 13;

const SWITCH_LETTERS = [
  { letter: "S", code: "KeyS" },
  { letter: "W", code: "KeyW" },
  { letter: "I", code: "KeyI" },
  { letter: "T", code: "KeyT" },
  { letter: "C", code: "KeyC" },
  { letter: "H", code: "KeyH" },
  { letter: "E", code: "KeyE" },
  { letter: "S", code: "KeyS" },
];

/**
 * Pure sizing calculator for Quick Actions collapsed & expanded states.
 * Clamps cleanly to constrained work-area heights while keeping the list scrollable.
 */
export function computeQuickActionsLayout({ expanded, pinCount, maxHeight = null }) {
  const count = Math.max(0, Math.floor(Number(pinCount) || 0));
  const naturalListHeight =
    count > 4
      ? QA_FOUR_PLUS_SLICE_HEIGHT
      : Math.max(QA_EMPTY_LIST_HEIGHT, count * QA_CARD_STRIDE - QA_CARD_GAP);

  if (!expanded) {
    return {
      shellHeight: QA_COLLAPSED_HEIGHT,
      panelHeight: 0,
      listHeight: naturalListHeight,
    };
  }

  const naturalShellHeight = QA_COLLAPSED_HEIGHT + naturalListHeight + QA_PANEL_EXTRA;
  const hasLimit = Number.isFinite(maxHeight) && maxHeight > 0;
  const clampedMax = hasLimit ? Math.max(QA_COLLAPSED_HEIGHT, Math.floor(maxHeight)) : naturalShellHeight;
  const shellHeight = Math.min(naturalShellHeight, clampedMax);
  const panelHeight = Math.max(0, shellHeight - QA_COLLAPSED_HEIGHT);
  const listHeight = Math.max(0, Math.min(naturalListHeight, panelHeight - QA_PANEL_EXTRA));

  return {
    shellHeight,
    panelHeight,
    listHeight,
  };
}

/**
 * Compute expressive, anatomically bounded Quick Actions eye & head gaze
 * transforms from cursor delta `(dx, dy)` relative to Ink's eye center.
 * Eyes rest at x=76..108, y=48..69 inside the 160x119 blob (deck covers y>=76).
 */
export function computeQuickActionsGaze(dx, dy) {
  const nx = Math.tanh(dx / 80);
  const ny = Math.tanh(dy / 72);
  const eyeX = nx < 0 ? nx * 32 : nx * 22;
  const eyeY = ny < 0 ? ny * 17 : ny * 9.5;
  const bodyX = nx * 5.2;
  const bodyY = ny * 2.6;
  const bodyTilt = nx * 3.5;
  const eyeSquashX = 1 - Math.abs(nx) * 0.14;
  return { eyeX, eyeY, bodyX, bodyY, bodyTilt, eyeSquashX };
}

export async function mountQuickActions(root) {
  document.title = "Thocci Quick Actions";
  document.body.className = "mode-quick-actions";
  document.documentElement.setAttribute("data-theme", "light");

  const mascotSvg = await loadTextAsset("./vendor/mascot/thocci-quick-actions.svg");

  root.innerHTML = `
    <div class="qa-viewport">
      <section class="qa-shell" role="dialog" aria-label="Thocci Quick Actions">
        <h1 class="qa-wordmark"><img src="./vendor/brand/thocci-wordmark.svg" width="388" height="99" alt="Thocci" draggable="false" /></h1>
        <div class="qa-mascot" aria-hidden="true">${mascotSvg}</div>
        <div class="qa-deck">
          <button
            id="qaToggleBtn"
            type="button"
            class="qa-keycap qa-toggle"
            aria-label="Toggle Thocci keyboard sounds"
            aria-pressed="false"
          >
            ${iconMarkup("keyboard-off")}
          </button>
          <div class="qa-status-wrap" aria-live="polite">
            <span id="qaStatusText" class="qa-status-text">Thocci is off</span>
            <span id="qaStatusSub" class="qa-status-sub" hidden></span>
          </div>
          <button
            id="qaGearBtn"
            type="button"
            class="qa-keycap qa-gear"
            aria-label="Settings"
          >
            ${iconMarkup("settings")}
          </button>
          <button
            id="qaSwitchesRow"
            type="button"
            class="qa-switches-row"
            aria-label="Browse soundpacks"
          >
            ${SWITCH_LETTERS.map(
              (item, idx) =>
                `<span class="qa-switch-cap" data-idx="${idx}" data-code="${item.code}" data-pressed="false" aria-hidden="true">${item.letter}</span>`,
            ).join("")}
          </button>
          <button
            id="qaPinnedToggle"
            class="qa-keycap qa-pinned-toggle"
            type="button"
            aria-expanded="false"
            aria-controls="qaPinnedPanel"
          >
            ${iconMarkup("pin-filled")}
            <span>Pinned</span>
            ${iconMarkup("chevron-right", "qa-pinned-chevron")}
          </button>
          <div id="qaPinnedPanel" class="qa-pinned-panel" data-theme-scope="dark" aria-hidden="true" inert>
            <div id="qaPinnedList" class="qa-pinned-list" role="region" aria-label="Pinned soundpacks" tabindex="0"></div>
          </div>
        </div>
      </section>
    </div>
  `;

  const viewport = root.querySelector(".qa-viewport");
  const shell = root.querySelector(".qa-shell");
  const toggleBtn = root.querySelector("#qaToggleBtn");
  const statusText = root.querySelector("#qaStatusText");
  const statusSub = root.querySelector("#qaStatusSub");
  const gearBtn = root.querySelector("#qaGearBtn");
  const switchesRow = root.querySelector("#qaSwitchesRow");
  const switchCaps = Array.from(root.querySelectorAll(".qa-switch-cap"));
  const pinnedToggle = root.querySelector("#qaPinnedToggle");
  const pinnedPanel = root.querySelector("#qaPinnedPanel");
  const pinnedList = root.querySelector("#qaPinnedList");
  const bodyGroup = root.querySelector("#thocci-body");
  const eyesGroup = root.querySelector("#thocci-eyes");

  const reducedMotionQuery = window.matchMedia("(prefers-reduced-motion: reduce)");

  let layerBackdropMode = false;
  let closingPopup = false;
  let expanded = false;
  let maxAvailableHeight = null;
  let lastNativeHeight = QA_COLLAPSED_HEIGHT;
  let shrinkTimer = null;
  let switchingPack = false;
  let pinMutationChain = Promise.resolve();

  let engineStatus = {
    running: false,
    responsive: false,
    volume: 100,
    pack: "cherry-mx",
  };
  let catalog = [];
  let prefs = { pinned_packs: [] };
  let toggling = false;

  function playbackReady(st = engineStatus) {
    return Boolean(st.running && st.responsive !== false);
  }

  function showInlineError(msg) {
    if (msg) {
      statusSub.hidden = false;
      statusSub.textContent = msg;
      statusSub.title = msg;
    } else {
      statusSub.hidden = true;
      statusSub.textContent = "";
      statusSub.title = "";
    }
  }

  function syncNativeHeight(targetHeight) {
    if (shrinkTimer) {
      clearTimeout(shrinkTimer);
      shrinkTimer = null;
    }
    const desired = Math.max(QA_COLLAPSED_HEIGHT, Math.round(targetHeight));
    if (desired === lastNativeHeight) return;

    const applyNow = () => {
      shrinkTimer = null;
      lastNativeHeight = desired;
      void api
        .setQuickActionsHeight(desired)
        .then((applied) => {
          if (typeof applied === "number" && applied > 0 && applied < desired) {
            maxAvailableHeight = applied;
            renderPinned({ syncNative: false });
          }
        })
        .catch(() => {});
    };

    // Expand immediately so the native window / layer-shell never clips incoming cards.
    // On collapse in non-layer-shell floating windows, wait for the 225ms CSS transition
    // unless reduced motion is requested, then shrink promptly to release pointer input.
    if (desired > lastNativeHeight || reducedMotionQuery.matches || layerBackdropMode) {
      applyNow();
    } else {
      shrinkTimer = setTimeout(applyNow, 230);
    }
  }

  function applyPlacement(placement) {
    if (!placement) return;
    if (typeof placement.maxHeight === "number" && placement.maxHeight >= QA_COLLAPSED_HEIGHT) {
      maxAvailableHeight = placement.maxHeight;
      renderPinned({ syncNative: false });
    }
    if (!placement.layerShell) return;
    layerBackdropMode = true;
    document.body.dataset.layerBackdrop = "true";
    shell.style.left = `${Math.round(placement.x)}px`;
    shell.style.top = `${Math.round(placement.y)}px`;
    shell.classList.remove("qa-closing");
    if (placement.animate !== false) {
      shell.classList.remove("qa-entering");
      void shell.offsetWidth;
      shell.classList.add("qa-entering");
    }
  }

  function dismissQuickActions() {
    if (closingPopup) return;
    if (!layerBackdropMode || reducedMotionQuery.matches) {
      void api.hideQuickActions().catch(() => {});
      return;
    }
    closingPopup = true;
    shell.classList.remove("qa-entering");
    shell.classList.add("qa-closing");
    setTimeout(() => {
      closingPopup = false;
      shell.classList.remove("qa-closing");
      void api.hideQuickActions().catch(() => {});
    }, 145);
  }

  let focusWasInPinnedPanel = false;

  // Prevent mouse clicks on Quick Actions buttons from leaving a sticky focus outline
  shell.addEventListener(
    "mousedown",
    (ev) => {
      if (ev.button !== 0) return;
      const btn = ev.target instanceof Element ? ev.target.closest("button") : null;
      if (btn) {
        focusWasInPinnedPanel =
          btn === pinnedToggle &&
          document.activeElement instanceof HTMLElement &&
          pinnedPanel.contains(document.activeElement);
        if (document.activeElement instanceof HTMLElement && document.activeElement !== document.body) {
          document.activeElement.blur();
        }
        ev.preventDefault();
      }
    },
    true,
  );

  // Click anywhere outside the popup card dismisses it (just like Calendar / Wi-Fi / Media popups)
  viewport.addEventListener("pointerdown", (ev) => {
    if (!layerBackdropMode) return;
    if (!shell.contains(ev.target)) {
      ev.preventDefault();
      dismissQuickActions();
    }
  });

  function renderPinned({ syncNative = true } = {}) {
    const packs = resolvePinnedPacks(prefs.pinned_packs, catalog);
    const selectDisabled = !playbackReady() || switchingPack;
    const contents = packs
      .map((pack) => renderPackCard(pack, engineStatus.pack, true, { disabled: selectDisabled }))
      .join("");

    replaceCards(
      pinnedList,
      contents ||
        `<div class="sp-empty"><p class="sp-empty-text">Pin a soundpack from the Soundpacks sidebar.</p></div>`,
      pinnedToggle,
    );

    const layout = computeQuickActionsLayout({
      expanded,
      pinCount: packs.length,
      maxHeight: maxAvailableHeight,
    });
    pinnedList.style.setProperty("--pinned-list-height", `${layout.listHeight}px`);
    shell.style.setProperty("--pinned-panel-height", expanded ? `${layout.panelHeight}px` : "0px");
    pinnedToggle.setAttribute("aria-expanded", String(expanded));
    pinnedPanel.setAttribute("aria-hidden", String(!expanded));
    pinnedPanel.inert = !expanded;

    if (syncNative) {
      syncNativeHeight(layout.shellHeight);
    }
  }

  pinnedToggle.addEventListener("click", () => {
    const focusInsidePanel =
      focusWasInPinnedPanel ||
      (document.activeElement instanceof HTMLElement && pinnedPanel.contains(document.activeElement));
    focusWasInPinnedPanel = false;
    expanded = !expanded;
    if (expanded) {
      pinnedList.scrollTop = 0;
    } else if (focusInsidePanel) {
      pinnedToggle.focus({ preventScroll: true });
    }
    renderPinned();
  });

  function unpinFromQuickActions(packId) {
    if (!packId) return;
    pinMutationChain = pinMutationChain.then(async () => {
      try {
        const result = await api.setPackPinned(packId, false);
        if (result?.prefs) {
          prefs = {
            ...prefs,
            ...result.prefs,
            pinned_packs: Array.isArray(result.prefs.pinned_packs)
              ? [...result.prefs.pinned_packs]
              : [],
          };
        }
        renderPinned();
      } catch (err) {
        const fresh = await api.getUiPrefs().catch(() => null);
        if (fresh) {
          prefs = {
            ...prefs,
            ...fresh,
            pinned_packs: Array.isArray(fresh.pinned_packs) ? [...fresh.pinned_packs] : [],
          };
        }
        renderPinned();
        showInlineError(formatShortError(err));
      }
    });
  }

  async function selectPackFromQuickActions(packId) {
    if (!packId || switchingPack || !playbackReady()) return;
    switchingPack = true;
    renderPinned({ syncNative: false });
    try {
      const confirmed = await api.setPack(packId);
      renderStatus(confirmed);
      if (playbackReady(confirmed)) {
        void api.playPreview(null, "down").catch(() => {});
      }
    } catch (err) {
      const fresh = await api.getStatus().catch(() => engineStatus);
      renderStatus(fresh, formatShortError(err));
    } finally {
      switchingPack = false;
      renderPinned({ syncNative: false });
    }
  }

  pinnedList.addEventListener("click", (event) => {
    const target = event.target instanceof Element ? event.target : null;
    if (!target) return;
    const pin = target.closest("[data-pin-id]");
    if (pin) {
      unpinFromQuickActions(pin.dataset.pinId);
      return;
    }
    const button = target.closest("[data-pack-id]");
    if (button && !button.disabled && playbackReady()) {
      void selectPackFromQuickActions(button.dataset.packId);
    }
  });

  function renderStatus(st, errorMsg = "") {
    engineStatus = st;
    const isOn = Boolean(st.running);
    const isReady = playbackReady(st);
    shell.dataset.thocciEnabled = isOn ? "true" : "false";
    switchesRow.disabled = !isReady;
    toggleBtn.setAttribute("aria-pressed", isOn ? "true" : "false");
    toggleBtn.setAttribute(
      "aria-label",
      isOn ? "Disable Thocci keyboard sounds" : "Enable Thocci keyboard sounds",
    );
    toggleBtn.innerHTML = iconMarkup(isOn ? "keyboard" : "keyboard-off");

    if (isOn && st.responsive === false) {
      statusText.textContent = "Thocci starting…";
    } else {
      statusText.textContent = isOn ? "Thocci is on" : "Thocci is off";
    }

    showInlineError(errorMsg);
    renderPinned({ syncNative: false });
  }

  async function reconcileAll() {
    try {
      const [st, freshPrefs, freshCatalog] = await Promise.all([
        api.getStatus(),
        api.getUiPrefs().catch(() => null),
        api.listPackCatalog().catch(() => null),
      ]);
      if (freshPrefs) {
        prefs = {
          ...prefs,
          ...freshPrefs,
          pinned_packs: Array.isArray(freshPrefs.pinned_packs) ? [...freshPrefs.pinned_packs] : [],
        };
      }
      if (Array.isArray(freshCatalog)) {
        catalog = freshCatalog;
      }
      renderStatus(st);
      if (expanded) {
        renderPinned({ syncNative: true });
      }
    } catch (err) {
      showInlineError(formatShortError(err));
    }
  }

  // Toggle keyboard keycap: plays an audition sample only when turning Thocci on and ready.
  toggleBtn.addEventListener("click", async () => {
    if (toggling) return;
    toggling = true;
    toggleBtn.disabled = true;
    toggleBtn.setAttribute("data-pressed", "true");
    showInlineError("");

    try {
      const next = await api.toggleDaemon();
      renderStatus(next);
      if (playbackReady(next)) {
        void api.playPreview(null, "down").catch(() => {});
      }
    } catch (err) {
      const fresh = await api.getStatus().catch(() => engineStatus);
      const shortErr = formatShortError(err);
      renderStatus(fresh, shortErr);
    } finally {
      setTimeout(() => toggleBtn.setAttribute("data-pressed", "false"), 90);
      toggling = false;
      toggleBtn.disabled = false;
    }
  });

  // Gear keycap -> open/focus dashboard & dismiss popover.
  gearBtn.addEventListener("click", async () => {
    gearBtn.setAttribute("data-pressed", "true");
    if (playbackReady()) {
      void api.playPreview(null, "down").catch(() => {});
    }
    setTimeout(async () => {
      gearBtn.setAttribute("data-pressed", "false");
      try {
        await api.openDashboard(null);
      } catch (err) {
        console.error("openDashboard failed:", err);
      }
    }, 65);
  });

  // SWITCHES row -> 8 visual caps with per-cap pressed feedback, 1 logical accessible action.
  let pressedCapIdx = null;
  let lastCapCode = "KeyS";

  function clearSwitchCapPress() {
    if (pressedCapIdx !== null && switchCaps[pressedCapIdx]) {
      switchCaps[pressedCapIdx].setAttribute("data-pressed", "false");
    }
    pressedCapIdx = null;
  }

  switchesRow.addEventListener("pointerdown", (ev) => {
    if (!playbackReady()) return;
    const cap = ev.target.closest?.(".qa-switch-cap");
    clearSwitchCapPress();
    if (cap) {
      const idx = Number(cap.dataset.idx);
      pressedCapIdx = idx;
      lastCapCode = cap.dataset.code || "KeyS";
      cap.setAttribute("data-pressed", "true");
    } else {
      lastCapCode = "KeyS";
    }
  });

  switchesRow.addEventListener("pointerup", clearSwitchCapPress);
  switchesRow.addEventListener("pointercancel", clearSwitchCapPress);
  switchesRow.addEventListener("pointerleave", clearSwitchCapPress);

  switchesRow.addEventListener("click", (ev) => {
    if (!playbackReady()) return;
    const cap = ev.target.closest?.(".qa-switch-cap");
    const code = cap?.dataset?.code || lastCapCode || "KeyS";
    const highlightCap = cap || switchCaps[0];
    if (highlightCap) {
      highlightCap.setAttribute("data-pressed", "true");
    }
    void api.playPreview(code, "down").catch(() => {});
    setTimeout(async () => {
      if (highlightCap) highlightCap.setAttribute("data-pressed", "false");
      try {
        await api.openDashboard("soundpacks");
      } catch (err) {
        console.error("openDashboard(soundpacks) failed:", err);
      }
    }, 65);
  });

  function getFocusableElements() {
    return Array.from(
      shell.querySelectorAll('button:not([disabled]), [tabindex]:not([tabindex="-1"])'),
    ).filter((el) => {
      if (el.closest('[inert], [hidden], [aria-hidden="true"]')) return false;
      return true;
    });
  }

  // Dynamic focus trap & Escape dismissal inside Quick Actions popover.
  function handleKeyDown(ev) {
    if (ev.key === "Escape") {
      ev.preventDefault();
      dismissQuickActions();
      return;
    }
    if (ev.key === "Tab") {
      const focusable = getFocusableElements();
      if (focusable.length === 0) return;
      const currentIdx = focusable.indexOf(document.activeElement);
      ev.preventDefault();
      if (ev.shiftKey) {
        const nextIdx = currentIdx <= 0 ? focusable.length - 1 : currentIdx - 1;
        focusable[nextIdx].focus();
      } else {
        const nextIdx = currentIdx < 0 || currentIdx >= focusable.length - 1 ? 0 : currentIdx + 1;
        focusable[nextIdx].focus();
      }
    }
  }
  window.addEventListener("keydown", handleKeyDown);

  // Expressive pointer-follow eye & subtle head tracking for Ink in Quick Actions
  let trackingActive = !document.hidden;
  let targetEyeX = 0;
  let targetEyeY = 0;
  let targetBodyX = 0;
  let targetBodyY = 0;
  let targetBodyTilt = 0;
  let targetEyeSquashX = 1;

  let curEyeX = 0;
  let curEyeY = 0;
  let curBodyX = 0;
  let curBodyY = 0;
  let curBodyTilt = 0;
  let curEyeSquashX = 1;
  let rafId = null;

  function applyGazeTransforms() {
    if (eyesGroup) {
      if (
        Math.abs(curEyeX) < 0.02 &&
        Math.abs(curEyeY) < 0.02 &&
        Math.abs(curEyeSquashX - 1) < 0.002
      ) {
        eyesGroup.removeAttribute("transform");
      } else {
        eyesGroup.setAttribute(
          "transform",
          `translate(${curEyeX.toFixed(2)}, ${curEyeY.toFixed(2)}) translate(92, 58) scale(${curEyeSquashX.toFixed(3)}, 1) translate(-92, -58)`,
        );
      }
    }
    if (bodyGroup) {
      if (
        Math.abs(curBodyX) < 0.02 &&
        Math.abs(curBodyY) < 0.02 &&
        Math.abs(curBodyTilt) < 0.02
      ) {
        bodyGroup.removeAttribute("transform");
      } else {
        bodyGroup.setAttribute(
          "transform",
          `translate(${curBodyX.toFixed(2)}, ${curBodyY.toFixed(2)}) rotate(${curBodyTilt.toFixed(2)}, 80, 119)`,
        );
      }
    }
  }

  function tickEyes() {
    if (!trackingActive || reducedMotionQuery.matches) {
      rafId = null;
      return;
    }
    const alpha = 0.28;
    curEyeX += (targetEyeX - curEyeX) * alpha;
    curEyeY += (targetEyeY - curEyeY) * alpha;
    curBodyX += (targetBodyX - curBodyX) * alpha;
    curBodyY += (targetBodyY - curBodyY) * alpha;
    curBodyTilt += (targetBodyTilt - curBodyTilt) * alpha;
    curEyeSquashX += (targetEyeSquashX - curEyeSquashX) * alpha;
    applyGazeTransforms();

    const settled =
      Math.abs(targetEyeX - curEyeX) < 0.04 &&
      Math.abs(targetEyeY - curEyeY) < 0.04 &&
      Math.abs(targetBodyX - curBodyX) < 0.03 &&
      Math.abs(targetBodyY - curBodyY) < 0.03 &&
      Math.abs(targetBodyTilt - curBodyTilt) < 0.03;

    if (!settled) {
      rafId = requestAnimationFrame(tickEyes);
    } else {
      curEyeX = targetEyeX;
      curEyeY = targetEyeY;
      curBodyX = targetBodyX;
      curBodyY = targetBodyY;
      curBodyTilt = targetBodyTilt;
      curEyeSquashX = targetEyeSquashX;
      applyGazeTransforms();
      rafId = null;
    }
  }

  function kickEyesLoop() {
    if (!trackingActive || reducedMotionQuery.matches) return;
    if (rafId === null) {
      rafId = requestAnimationFrame(tickEyes);
    }
  }

  function resetEyes(immediate = false) {
    targetEyeX = 0;
    targetEyeY = 0;
    targetBodyX = 0;
    targetBodyY = 0;
    targetBodyTilt = 0;
    targetEyeSquashX = 1;
    if (immediate || reducedMotionQuery.matches) {
      if (rafId !== null) {
        cancelAnimationFrame(rafId);
        rafId = null;
      }
      curEyeX = 0;
      curEyeY = 0;
      curBodyX = 0;
      curBodyY = 0;
      curBodyTilt = 0;
      curEyeSquashX = 1;
      applyGazeTransforms();
    } else {
      kickEyesLoop();
    }
  }

  function handlePointerMove(ev) {
    if (!trackingActive || reducedMotionQuery.matches) return;
    const rect = shell.getBoundingClientRect();
    // Mascot eye center sits around (156, 136) in popover coordinates
    const eyeCenterX = rect.left + 156;
    const eyeCenterY = rect.top + 136;
    const gaze = computeQuickActionsGaze(ev.clientX - eyeCenterX, ev.clientY - eyeCenterY);
    targetEyeX = gaze.eyeX;
    targetEyeY = gaze.eyeY;
    targetBodyX = gaze.bodyX;
    targetBodyY = gaze.bodyY;
    targetBodyTilt = gaze.bodyTilt;
    targetEyeSquashX = gaze.eyeSquashX;
    kickEyesLoop();
  }

  function handlePointerLeave() {
    resetEyes(false);
  }

  window.addEventListener("pointermove", handlePointerMove, { passive: true });
  window.addEventListener("mousemove", handlePointerMove, { passive: true });
  document.addEventListener("pointerleave", handlePointerLeave);
  document.addEventListener("mouseleave", handlePointerLeave);
  reducedMotionQuery.addEventListener("change", () => resetEyes(true));

  const unlistenPlacement = await listenEvent("thocci://quick-actions-placement", (placement) => {
    applyPlacement(placement);
  });

  const unlistenState = await listenEvent("thocci://state-changed", (st) => {
    if (st) renderStatus(st);
  });

  const unlistenPrefs = await listenEvent("thocci://prefs-changed", (nextPrefs) => {
    if (nextPrefs) {
      prefs = {
        ...prefs,
        ...nextPrefs,
        pinned_packs: Array.isArray(nextPrefs.pinned_packs) ? [...nextPrefs.pinned_packs] : [],
      };
      renderPinned();
    }
  });

  const unlistenCatalog = await listenEvent("thocci://catalog-changed", (nextCatalog) => {
    if (Array.isArray(nextCatalog)) {
      catalog = nextCatalog;
      renderPinned();
    }
  });

  const unlistenShown = await listenEvent("thocci://quick-actions-shown", () => {
    closingPopup = false;
    trackingActive = true;
    resetEyes(true);
    void api.getQuickActionsPlacement().then((p) => applyPlacement(p)).catch(() => {});
    void reconcileAll();
  });

  const unlistenHidden = await listenEvent("thocci://quick-actions-hidden", () => {
    closingPopup = false;
    trackingActive = false;
    resetEyes(true);
  });

  window.addEventListener("focus", () => {
    trackingActive = true;
    void reconcileAll();
  });

  document.addEventListener("visibilitychange", () => {
    trackingActive = !document.hidden;
    if (!trackingActive) {
      resetEyes(true);
    } else {
      void reconcileAll();
    }
  });

  const initialPlacement = await api.getQuickActionsPlacement().catch(() => null);
  applyPlacement(initialPlacement);
  await reconcileAll();

  return {
    destroy() {
      if (shrinkTimer) clearTimeout(shrinkTimer);
      trackingActive = false;
      resetEyes(true);
      window.removeEventListener("keydown", handleKeyDown);
      window.removeEventListener("pointermove", handlePointerMove);
      window.removeEventListener("mousemove", handlePointerMove);
      document.removeEventListener("pointerleave", handlePointerLeave);
      document.removeEventListener("mouseleave", handlePointerLeave);
      unlistenPlacement();
      unlistenState();
      unlistenPrefs();
      unlistenCatalog();
      unlistenShown();
      unlistenHidden();
    },
  };
}

function formatShortError(err) {
  const raw = String(err || "Could not update engine");
  if (/permission|input monitoring|input.*group/i.test(raw)) {
    return "Needs keyboard permission";
  }
  return raw.length > 32 ? `${raw.slice(0, 31)}…` : raw;
}
