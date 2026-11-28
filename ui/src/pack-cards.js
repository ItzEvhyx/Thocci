/**
 * Shared soundpack card & favourites helpers used by both the Dashboard
 * Soundpacks sidebar and the Quick Actions Pinned list.
 */

import { iconMarkup } from "./icons.js";

export const MAX_PINNED_PACKS = 10;
export const PIN_LIMIT_MESSAGE = "You can pin up to 10 soundpacks. Unpin one to make room.";

export function escapeMarkup(value) {
  return String(value ?? "").replace(/[&<>"']/g, (char) =>
    ({
      "&": "&amp;",
      "<": "&lt;",
      ">": "&gt;",
      '"': "&quot;",
      "'": "&#39;",
    })[char],
  );
}

/**
 * Resolve stored pinned IDs against the current catalog, deduplicating and
 * preserving pin order while safely ignoring stale/missing IDs.
 */
export function resolvePinnedPacks(pinnedIds, catalog) {
  if (!Array.isArray(pinnedIds) || !Array.isArray(catalog)) {
    return [];
  }
  const byId = new Map(catalog.map((pack) => [pack.id, pack]));
  const seen = new Set();
  const resolved = [];
  for (const rawId of pinnedIds) {
    if (typeof rawId !== "string") continue;
    const id = rawId.trim();
    if (!id || seen.has(id)) continue;
    const pack = byId.get(id);
    if (!pack) continue;
    seen.add(id);
    resolved.push(pack);
    if (resolved.length >= MAX_PINNED_PACKS) break;
  }
  return resolved;
}

/**
 * Render one soundpack card with sibling select and pin/unpin buttons.
 */
export function renderPackCard(pack, activeId, isPinned, { disabled = false, pinDisabled = false } = {}) {
  const id = escapeMarkup(pack.id);
  const name = escapeMarkup(pack.display_name);
  const subtitle = escapeMarkup(pack.subtitle);
  const active = pack.id === activeId;
  const pinAction = isPinned ? "Unpin" : "Pin";
  return `
    <div class="pack-card" data-active="${active}">
      <button type="button" class="sp-item" data-pack-id="${id}"
              aria-current="${active}" aria-label="${name}"
              title="${name}" ${disabled ? "disabled" : ""}>
        <span class="sp-item-badge" aria-hidden="true">${iconMarkup("keyboard")}</span>
        <span class="sp-item-copy">
          <span class="sp-item-name">${name}</span>
          <span class="sp-item-sub">${subtitle}</span>
        </span>
        ${active ? `<span class="sp-item-check" aria-label="Active soundpack">${iconMarkup("check")}</span>` : ""}
      </button>
      <button type="button" class="pack-pin" data-pin-id="${id}"
              aria-pressed="${isPinned}" aria-label="${pinAction} ${name}"
              title="${pinAction} ${name}" ${pinDisabled ? "disabled" : ""}>
        ${iconMarkup(isPinned ? "pin-filled" : "pin")}
      </button>
    </div>
  `;
}

/**
 * Replace card list markup in place while preserving scroll position and
 * restoring keyboard focus cleanly (even when the focused card was removed).
 */
export function replaceCards(list, markup, fallback = null) {
  const active = document.activeElement;
  const inside = Boolean(active && list.contains(active) && active !== list);
  const scrollTop = list.scrollTop;
  let sectionLabel = null;
  let attr = null;
  let value = null;
  let previousIndex = -1;

  if (inside) {
    sectionLabel = active.closest(".sp-section")?.getAttribute("aria-label") ?? null;
    attr = active.hasAttribute("data-pin-id") ? "data-pin-id" : "data-pack-id";
    value = active.getAttribute(attr);
    const prevButtons = Array.from(list.querySelectorAll(`[${attr}]`)).filter(
      (btn) => (btn.closest(".sp-section")?.getAttribute("aria-label") ?? null) === sectionLabel,
    );
    previousIndex = prevButtons.indexOf(active);
  }

  list.innerHTML = markup;
  list.scrollTop = scrollTop;

  if (!inside) return;

  const escapedVal = typeof CSS !== "undefined" && typeof CSS.escape === "function"
    ? CSS.escape(value ?? "")
    : String(value ?? "").replace(/["\\]/g, "\\$&");
  const candidates = Array.from(list.querySelectorAll(`[${attr}="${escapedVal}"]`));
  const sameSection = candidates.find(
    (btn) => (btn.closest(".sp-section")?.getAttribute("aria-label") ?? null) === sectionLabel,
  );

  let next = sameSection;
  if (!next && sectionLabel === null && previousIndex >= 0) {
    // In a single flat list (Quick Actions Pinned), prefer the neighbor at the same slot
    // before falling back to the toggle button.
    const remaining = Array.from(list.querySelectorAll(`[${attr}]:not([disabled])`));
    if (remaining.length > 0) {
      next = remaining[Math.min(previousIndex, remaining.length - 1)];
    }
  }
  if (!next) {
    next =
      candidates.find((btn) => !btn.disabled) ||
      list.querySelector(`[${attr}]:not([disabled])`) ||
      list.querySelector("button:not([disabled])") ||
      fallback;
  }

  next?.focus?.({ preventScroll: true });
  if (next && next !== fallback && list.contains(next)) {
    const cardBox = next.closest(".pack-card")?.getBoundingClientRect() ?? next.getBoundingClientRect();
    const listBox = list.getBoundingClientRect();
    if (cardBox.top < listBox.top || cardBox.bottom > listBox.bottom) {
      next.scrollIntoView({ block: "nearest" });
    }
  }
}
