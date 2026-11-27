/**
 * First-launch Welcome / Onboarding surface (480 x 474).
 */

import { api, currentTauriWindow, listenEvent } from "./bridge.js";
import { iconMarkup } from "./icons.js";

export async function mountOnboarding(root, { onComplete }) {
  document.body.className = "mode-onboarding";
  document.documentElement.setAttribute("data-theme", "light");

  // Fetch Wayland layer-shell rect before mounting so the card never flashes fullscreen.
  const initialPlacement = await api.getOverlayPlacement().catch(() => null);
  let layerPanelMode = false;
  let layerRect = { x: 720, y: 303, width: 480, height: 474, screenWidth: 1920, screenHeight: 1080 };
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
  const initialStyle = layerPanelMode
    ? ` style="left:${layerRect.x}px;top:${layerRect.y}px;width:${layerRect.width}px;height:${layerRect.height}px"`
    : "";

  root.innerHTML = `
    <main class="onboarding-shell is-entering"${initialStyle} data-tauri-drag-region aria-label="Welcome to Thocci">
      <button id="onboardingCloseBtn" type="button" class="onboarding-close-btn" aria-label="Close Thocci">×</button>
      <div class="onboarding-illustration-frame" data-tauri-drag-region>
        <img src="./vendor/illustrations/onboarding.svg" alt="Thocci perched on a black spacebar between Command and K keycaps" draggable="false" />
      </div>
      <div class="onboarding-copy" data-tauri-drag-region>
        <h1 class="onboarding-title">Welcome to <img class="onboarding-wordmark" src="./vendor/brand/thocci-wordmark.svg" width="388" height="99" alt="Thocci" draggable="false" /></h1>
        <p class="onboarding-tagline">Thocci - the keyboard you can hear.</p>
        <button id="onboardingGetStarted" type="button" class="onboarding-cta">
          <span>Get started</span>
          ${iconMarkup("arrow-right")}
        </button>
      </div>
    </main>
  `;

  const cta = root.querySelector("#onboardingGetStarted");
  const closeBtn = root.querySelector("#onboardingCloseBtn");
  const shell = root.querySelector(".onboarding-shell");
  let busy = false;
  setTimeout(() => shell?.classList.remove("is-entering"), 420);

  function applyLayerPanelRect() {
    if (!layerPanelMode) return;
    shell.style.left = `${layerRect.x}px`;
    shell.style.top = `${layerRect.y}px`;
    shell.style.width = `${layerRect.width}px`;
    shell.style.height = `${layerRect.height}px`;
  }

  function applyPlacement(placement) {
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

  let unlistenOverlay = () => {};
  void listenEvent("thocci://overlay-placement", applyPlacement).then((fn) => {
    unlistenOverlay = fn;
  });

  shell.addEventListener("pointerdown", (ev) => {
    if (ev.button !== 0 || ev.target.closest("button")) return;
    if (layerPanelMode) {
      ev.preventDefault();
      const startX = layerRect.x;
      const startY = layerRect.y;
      const downX = ev.clientX;
      const downY = ev.clientY;
      const screenW = layerRect.screenWidth || window.innerWidth || 1920;
      const screenH = layerRect.screenHeight || window.innerHeight || 1080;
      void api.setOverlayInputFullscreen(true).catch(() => {});
      try {
        shell.setPointerCapture(ev.pointerId);
      } catch {
        // Ignore
      }
      const onMove = (moveEv) => {
        layerRect.x = Math.max(
          -layerRect.width + 120,
          Math.min(screenW - 120, Math.round(startX + (moveEv.clientX - downX))),
        );
        layerRect.y = Math.max(
          0,
          Math.min(screenH - 60, Math.round(startY + (moveEv.clientY - downY))),
        );
        applyLayerPanelRect();
      };
      const onEnd = () => {
        window.removeEventListener("pointermove", onMove);
        window.removeEventListener("pointerup", onEnd);
        window.removeEventListener("pointercancel", onEnd);
        void api
          .setOverlayRect(layerRect.x, layerRect.y, layerRect.width, layerRect.height, false)
          .catch(() => {});
      };
      window.addEventListener("pointermove", onMove);
      window.addEventListener("pointerup", onEnd);
      window.addEventListener("pointercancel", onEnd);
    } else {
      void currentTauriWindow()?.startDragging?.().catch(() => {});
    }
  });

  closeBtn.addEventListener("click", () => {
    void currentTauriWindow()?.close?.().catch(() => {});
  });

  async function finish() {
    if (busy) return;
    busy = true;
    cta.disabled = true;
    try {
      void api.playPreview("Space", "down").catch(() => {});
      const prefs = await api.completeOnboarding();
      await onComplete(prefs);
    } catch (err) {
      busy = false;
      cta.disabled = false;
      console.error("Failed to complete onboarding:", err);
    }
  }

  cta.addEventListener("click", finish);
  cta.focus();

  return {
    destroy() {
      unlistenOverlay();
      cta.removeEventListener("click", finish);
    },
  };
}
