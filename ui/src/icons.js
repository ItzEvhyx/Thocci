/**
 * Bundled SVG loader & cache for icons, mascot layers, and keyboard glyphs.
 */

const textCache = new Map();

export async function loadTextAsset(relPath) {
  if (textCache.has(relPath)) {
    return textCache.get(relPath);
  }
  const resp = await fetch(relPath);
  if (!resp.ok) {
    throw new Error(`Failed to load asset ${relPath}: ${resp.status}`);
  }
  const text = await resp.text();
  textCache.set(relPath, text);
  return text;
}

export async function loadJsonAsset(relPath) {
  const raw = await loadTextAsset(relPath);
  return JSON.parse(raw);
}

const UI_ICONS = [
  "animate",
  "arrow-right",
  "back",
  "check",
  "chevron-right",
  "dark-mode",
  "import-wav",
  "keyboard-off",
  "keyboard",
  "light-mode",
  "power",
  "search",
  "settings",
  "setup-guide",
  "volume",
  "pin",
  "pin-filled",
];

const iconMap = new Map();

export async function preloadUiIcons() {
  await Promise.all(
    UI_ICONS.map(async (name) => {
      const svg = await loadTextAsset(`./vendor/icons/${name}.svg`);
      iconMap.set(name, svg);
    }),
  );
}

export function iconMarkup(name, extraClass = "") {
  const svg = iconMap.get(name) ?? "";
  const cls = extraClass ? `ui-icon ${extraClass}` : "ui-icon";
  return `<span class="${cls}" aria-hidden="true">${svg}</span>`;
}
