/**
 * Thocci Desktop UI entry point.
 *
 * Routes `?view=quick-actions` to the compact tray popover and the main window
 * to either First-Launch Onboarding or the full Desktop Dashboard.
 */

import { api } from "./bridge.js";
import { mountDashboard } from "./dashboard.js";
import { preloadUiIcons } from "./icons.js";
import { mountOnboarding } from "./onboarding.js";
import { mountQuickActions } from "./quick-actions.js";

async function boot() {
  const root = document.getElementById("app");
  if (!root) return;

  await preloadUiIcons();

  const params = new URLSearchParams(window.location.search);
  const view = params.get("view");

  if (view === "quick-actions") {
    await mountQuickActions(root);
    return;
  }

  const prefs = await api.getUiPrefs();
  if (
    view === "onboarding" ||
    (!prefs.onboarding_completed && view !== "dashboard" && view !== "soundpacks")
  ) {
    await mountOnboarding(root, {
      async onComplete(updatedPrefs) {
        await mountDashboard(root, { initialPrefs: updatedPrefs });
      },
    });
    return;
  }

  await mountDashboard(root, {
    initialPrefs: prefs,
    initialRoute: view === "soundpacks" ? "soundpacks" : null,
  });
}

boot().catch((err) => {
  const root = document.getElementById("app");
  if (root) {
    root.innerHTML = `<div style="padding:24px;font-family:sans-serif;color:#b91c1c">Failed to initialize Thocci UI: ${String(err)}</div>`;
  }
  console.error(err);
});
