/**
 * Quick Actions tray / menu-bar popover (288 x 282).
 *
 * - Wordmark "Thocci", layered mascot behind black deck with bounded
 *   smooth pointer-follow eyes (#thocci-eyes).
 * - Left keyboard keycap toggles engine playback, updates status after
 *   confirmation ("Thocci is on" / "Thocci is off"), keeps popover open.
 * - Right gear keycap ("Settings") opens/focuses dashboard and dismisses popover.
 * - Bottom 8 individual "S W I T C H E S" keycaps share one accessible
 *   "Browse soundpacks" control, show individual cap pressed feedback, play
 *   matching letter audition, and route directly to the Soundpacks sidebar.
 */

import { api, listenEvent } from "./bridge.js";
import { iconMarkup, loadTextAsset } from "./icons.js";

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

export async function mountQuickActions(root) {
  document.title = "Thocci Quick Actions";
  document.body.className = "mode-quick-actions";
  document.documentElement.setAttribute("data-theme", "light");

  const mascotSvg = await loadTextAsset("./vendor/mascot/thocci-quick-actions.svg");

  root.innerHTML = `
    <div class="qa-viewport">
      <section class="qa-shell" role="dialog" aria-label="Thocci Quick Actions">
        <h1 class="qa-wordmark">Thocci</h1>
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
  const eyesGroup = root.querySelector("#thocci-eyes");

  const focusable = [toggleBtn, gearBtn, switchesRow];
  const reducedMotionQuery = window.matchMedia("(prefers-reduced-motion: reduce)");

  let layerBackdropMode = false;
  let closingPopup = false;

  function applyPlacement(placement) {
    if (!placement || !placement.layerShell) return;
    layerBackdropMode = true;
    document.body.dataset.layerBackdrop = "true";
    shell.style.left = `${Math.round(placement.x)}px`;
    shell.style.top = `${Math.round(placement.y)}px`;
    shell.classList.remove("qa-closing");
    // Retrigger smooth pop-in animation (matching desktop widget popups)
    shell.classList.remove("qa-entering");
    void shell.offsetWidth;
    shell.classList.add("qa-entering");
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

  // Click anywhere outside the popup card dismisses it (just like Calendar / Wi-Fi / Media popups)
  viewport.addEventListener("pointerdown", (ev) => {
    if (!layerBackdropMode) return;
    if (!shell.contains(ev.target)) {
      ev.preventDefault();
      dismissQuickActions();
    }
  });

  let engineStatus = {
    running: false,
    responsive: false,
    volume: 100,
    pack: "cherry-mx",
  };
  let toggling = false;

  function renderStatus(st, errorMsg = "") {
    engineStatus = st;
    const isOn = Boolean(st.running);
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

    if (errorMsg) {
      statusSub.hidden = false;
      statusSub.textContent = errorMsg;
      statusSub.title = errorMsg;
    } else {
      statusSub.hidden = true;
      statusSub.textContent = "";
    }
  }

  async function reconcileStatus() {
    try {
      const st = await api.getStatus();
      renderStatus(st);
    } catch (err) {
      statusSub.hidden = false;
      statusSub.textContent = String(err);
    }
  }

  // Toggle keyboard keycap: plays one audition sample even when turning off / while off.
  toggleBtn.addEventListener("click", async () => {
    if (toggling) return;
    toggling = true;
    toggleBtn.disabled = true;
    toggleBtn.setAttribute("data-pressed", "true");
    statusSub.hidden = true;

    void api.playPreview(null, "down").catch(() => {});

    try {
      const next = await api.toggleDaemon();
      renderStatus(next);
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
    void api.playPreview(null, "down").catch(() => {});
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

  // Focus trap & Escape dismissal inside Quick Actions popover.
  function handleKeyDown(ev) {
    if (ev.key === "Escape") {
      ev.preventDefault();
      dismissQuickActions();
      return;
    }
    if (ev.key === "Tab") {
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

  // Bounded smooth pointer-follow eye tracking for #thocci-eyes
  let trackingActive = !document.hidden;
  let targetX = 0;
  let targetY = 0;
  let currentX = 0;
  let currentY = 0;
  let rafId = null;

  function applyEyesTransform(dx, dy) {
    if (!eyesGroup) return;
    if (Math.abs(dx) < 0.02 && Math.abs(dy) < 0.02) {
      eyesGroup.removeAttribute("transform");
    } else {
      eyesGroup.setAttribute("transform", `translate(${dx.toFixed(2)}, ${dy.toFixed(2)})`);
    }
  }

  function tickEyes() {
    if (!trackingActive || reducedMotionQuery.matches) {
      rafId = null;
      return;
    }
    currentX += (targetX - currentX) * 0.18;
    currentY += (targetY - currentY) * 0.18;
    applyEyesTransform(currentX, currentY);

    if (Math.abs(targetX - currentX) > 0.04 || Math.abs(targetY - currentY) > 0.04) {
      rafId = requestAnimationFrame(tickEyes);
    } else {
      currentX = targetX;
      currentY = targetY;
      applyEyesTransform(currentX, currentY);
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
    targetX = 0;
    targetY = 0;
    if (immediate || reducedMotionQuery.matches) {
      if (rafId !== null) {
        cancelAnimationFrame(rafId);
        rafId = null;
      }
      currentX = 0;
      currentY = 0;
      applyEyesTransform(0, 0);
    } else {
      kickEyesLoop();
    }
  }

  function handlePointerMove(ev) {
    if (!trackingActive || reducedMotionQuery.matches) return;
    const rect = shell.getBoundingClientRect();
    // Mascot eye center sits around (156, 136) in popover 288x282 coordinates
    const eyeCenterX = rect.left + 156;
    const eyeCenterY = rect.top + 136;
    const normX = Math.max(-1, Math.min(1, (ev.clientX - eyeCenterX) / 130));
    const normY = Math.max(-1, Math.min(1, (ev.clientY - eyeCenterY) / 115));
    // Clamp strictly inside the mascot face
    targetX = normX * 7.5;
    targetY = normY * 4.8;
    kickEyesLoop();
  }

  function handlePointerLeave() {
    resetEyes(false);
  }

  viewport.addEventListener("pointermove", handlePointerMove);
  viewport.addEventListener("pointerleave", handlePointerLeave);
  reducedMotionQuery.addEventListener("change", () => resetEyes(true));

  const unlistenPlacement = await listenEvent("thocci://quick-actions-placement", (placement) => {
    applyPlacement(placement);
  });

  const unlistenState = await listenEvent("thocci://state-changed", (st) => {
    if (st) renderStatus(st);
  });

  const unlistenShown = await listenEvent("thocci://quick-actions-shown", () => {
    closingPopup = false;
    trackingActive = true;
    resetEyes(true);
    void api.getQuickActionsPlacement().then((p) => applyPlacement(p)).catch(() => {});
    void reconcileStatus();
    toggleBtn.focus();
  });

  const unlistenHidden = await listenEvent("thocci://quick-actions-hidden", () => {
    closingPopup = false;
    trackingActive = false;
    resetEyes(true);
  });

  window.addEventListener("focus", () => {
    trackingActive = true;
    void reconcileStatus();
  });

  document.addEventListener("visibilitychange", () => {
    trackingActive = !document.hidden;
    if (!trackingActive) {
      resetEyes(true);
    } else {
      void reconcileStatus();
    }
  });

  const initialPlacement = await api.getQuickActionsPlacement().catch(() => null);
  applyPlacement(initialPlacement);
  await reconcileStatus();
  toggleBtn.focus();

  return {
    destroy() {
      trackingActive = false;
      resetEyes(true);
      window.removeEventListener("keydown", handleKeyDown);
      viewport.removeEventListener("pointermove", handlePointerMove);
      viewport.removeEventListener("pointerleave", handlePointerLeave);
      unlistenPlacement();
      unlistenState();
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
