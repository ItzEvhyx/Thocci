/**
 * Interactive Modern MacBook / Apple Magic Keyboard (ANSI 78-key) renderer & input controller.
 *
 * - Full-height modern function row (Esc, F1-F12 with macOS system glyphs, Touch ID Lock),
 *   ANSI number/alpha/modifier rows with horizontal Return and Globe key, and split Up/Down arrows.
 * - Physical keydown/keyup maps by `KeyboardEvent.code`, supports multi-key chords,
 *   clears held states on blur/hide/suspend, and NEVER plays duplicate audio
 *   (global sound comes exclusively from the Rust daemon when enabled).
 * - Pointer/touch interactions depress/release individual keys and audition the
 *   active soundpack via `onPointerAudition(code, "down" | "up")` even when
 *   background playback is disabled.
 */

const SVG_NS = "http://www.w3.org/2000/svg";

function svgEl(tag, attrs = {}) {
  const el = document.createElementNS(SVG_NS, tag);
  for (const [k, v] of Object.entries(attrs)) {
    el.setAttribute(k, String(v));
  }
  return el;
}

/**
 * Keys that are ignored for daemon sound playback in `thocci-core/src/keymap.rs`
 * (still visually highlighted on physical press, and audibly testable via pointer).
 */
export const IGNORED_PHYSICAL_AUDIO_CODES = new Set([
  "CapsLock",
  "NumLock",
  "ScrollLock",
  "MetaLeft",
  "MetaRight",
]);

/**
 * Pure key-hold tracker usable both in the browser UI and Node unit tests.
 */
export class KeyHoldTracker {
  constructor() {
    this.physical = new Set();
    this.pointers = new Map(); // pointerId -> code
  }

  physicalDown(code) {
    if (!code || this.physical.has(code)) {
      return false;
    }
    this.physical.add(code);
    return true;
  }

  physicalUp(code) {
    return this.physical.delete(code);
  }

  pointerDown(pointerId, code) {
    if (!code) return null;
    const prev = this.pointers.get(pointerId);
    this.pointers.set(pointerId, code);
    return prev && prev !== code ? prev : null;
  }

  pointerUp(pointerId) {
    const code = this.pointers.get(pointerId) ?? null;
    this.pointers.delete(pointerId);
    return code;
  }

  isPressed(code) {
    if (this.physical.has(code)) return true;
    for (const held of this.pointers.values()) {
      if (held === code) return true;
    }
    return false;
  }

  clearPhysical() {
    const cleared = Array.from(this.physical);
    this.physical.clear();
    return cleared;
  }

  clearAll() {
    const cleared = new Set([...this.physical, ...this.pointers.values()]);
    this.physical.clear();
    this.pointers.clear();
    return Array.from(cleared);
  }
}

/**
 * Crisp thin-stroke macOS modern Magic Keyboard glyphs (16x16 viewBox).
 */
const MAC_GLYPHS = {
  brightnessDown: `
    <circle cx="8" cy="8" r="2.2" fill="none" stroke="currentColor" stroke-width="1.05"/>
    <path d="M8 3.6v1.1M8 11.3v1.1M3.6 8h1.1M11.3 8h1.1M4.9 4.9l.8.8M10.3 10.3l.8.8M11.1 4.9l-.8.8M5.7 10.3l-.8.8" stroke="currentColor" stroke-width="1.05" stroke-linecap="round"/>
  `,
  brightnessUp: `
    <circle cx="8" cy="8" r="2.4" fill="none" stroke="currentColor" stroke-width="1.05"/>
    <path d="M8 2.2v1.8M8 12v1.8M2.2 8h1.8M12 8h1.8M3.9 3.9l1.3 1.3M10.8 10.8l1.3 1.3M12.1 3.9l-1.3 1.3M5.2 10.8l-1.3 1.3" stroke="currentColor" stroke-width="1.05" stroke-linecap="round"/>
  `,
  missionControl: `
    <rect x="2.2" y="3.8" width="5.2" height="3.8" rx="0.8" fill="none" stroke="currentColor" stroke-width="1.0"/>
    <rect x="9.0" y="3.8" width="4.8" height="3.8" rx="0.8" fill="none" stroke="currentColor" stroke-width="1.0"/>
    <rect x="4.6" y="9.1" width="6.8" height="3.2" rx="0.8" fill="none" stroke="currentColor" stroke-width="1.0"/>
  `,
  spotlight: `
    <circle cx="7.1" cy="7.1" r="3.6" fill="none" stroke="currentColor" stroke-width="1.1"/>
    <path d="M9.9 9.9L13.3 13.3" stroke="currentColor" stroke-width="1.15" stroke-linecap="round"/>
  `,
  dictation: `
    <rect x="6.1" y="2.6" width="3.8" height="6.4" rx="1.9" fill="none" stroke="currentColor" stroke-width="1.05"/>
    <path d="M4.2 7.6a3.8 3.8 0 0 0 7.6 0M8 11.4v2.2M6.0 13.6h4.0" fill="none" stroke="currentColor" stroke-width="1.05" stroke-linecap="round"/>
  `,
  dnd: `
    <path d="M11.6 9.9A4.4 4.4 0 0 1 6.1 4.4a4.4 4.4 0 1 0 5.5 5.5Z" fill="none" stroke="currentColor" stroke-width="1.05" stroke-linejoin="round"/>
  `,
  rewind: `
    <path d="M7.6 5.0L3.0 8.0l4.6 3.0V5.0ZM13.0 5.0L8.4 8.0l4.6 3.0V5.0Z" fill="none" stroke="currentColor" stroke-width="1.0" stroke-linejoin="round"/>
  `,
  playPause: `
    <path d="M2.8 4.8L7.8 8.0l-5.0 3.2V4.8Z" fill="none" stroke="currentColor" stroke-width="1.0" stroke-linejoin="round"/>
    <path d="M10.2 5.0v6.0M12.8 5.0v6.0" stroke="currentColor" stroke-width="1.1" stroke-linecap="round"/>
  `,
  fastForward: `
    <path d="M3.0 5.0L7.6 8.0l-4.6 3.0V5.0ZM8.4 5.0L13.0 8.0l-4.6 3.0V5.0Z" fill="none" stroke="currentColor" stroke-width="1.0" stroke-linejoin="round"/>
  `,
  mute: `
    <path d="M4.2 6.4h2.2l3.1-2.5v8.2l-3.1-2.5H4.2V6.4Z" fill="none" stroke="currentColor" stroke-width="1.05" stroke-linejoin="round"/>
  `,
  volumeDown: `
    <path d="M3.4 6.4h2.1l3.0-2.5v8.2l-3.0-2.5H3.4V6.4Z" fill="none" stroke="currentColor" stroke-width="1.05" stroke-linejoin="round"/>
    <path d="M10.8 6.2a2.5 2.5 0 0 1 0 3.6" fill="none" stroke="currentColor" stroke-width="1.05" stroke-linecap="round"/>
  `,
  volumeUp: `
    <path d="M2.6 6.4h2.0l2.9-2.5v8.2l-2.9-2.5H2.6V6.4Z" fill="none" stroke="currentColor" stroke-width="1.05" stroke-linejoin="round"/>
    <path d="M9.8 6.2a2.5 2.5 0 0 1 0 3.6M11.6 4.7a4.6 4.6 0 0 1 0 6.6" fill="none" stroke="currentColor" stroke-width="1.05" stroke-linecap="round"/>
  `,
  lock: `
    <rect x="4.3" y="7.2" width="7.4" height="5.8" rx="1.2" fill="none" stroke="currentColor" stroke-width="1.1"/>
    <path d="M5.9 7.2V5.4a2.1 2.1 0 0 1 4.2 0v1.8" fill="none" stroke="currentColor" stroke-width="1.1" stroke-linecap="round"/>
  `,
  globe: `
    <circle cx="8" cy="8" r="5.6" fill="none" stroke="currentColor" stroke-width="0.95"/>
    <ellipse cx="8" cy="8" rx="2.6" ry="5.6" fill="none" stroke="currentColor" stroke-width="0.9"/>
    <path d="M2.7 6.0h10.6M2.7 10.0h10.6M8 2.4v11.2" fill="none" stroke="currentColor" stroke-width="0.85"/>
  `,
  control: `
    <path d="M4.2 10.2L8.0 5.8l3.8 4.4" fill="none" stroke="currentColor" stroke-width="1.15" stroke-linecap="round" stroke-linejoin="round"/>
  `,
  option: `
    <path d="M2.6 5.2h3.0l4.2 5.6h3.6M9.6 5.2h3.8" fill="none" stroke="currentColor" stroke-width="1.1" stroke-linecap="round" stroke-linejoin="round"/>
  `,
  command: `
    <path d="M5.8 5.8V4.4a1.5 1.5 0 1 0-1.5 1.4h7.4a1.5 1.5 0 1 0-1.5-1.4v7.2a1.5 1.5 0 1 0 1.5-1.4H4.3a1.5 1.5 0 1 0 1.5 1.4V5.8Z" fill="none" stroke="currentColor" stroke-width="1.05" stroke-linejoin="round"/>
  `,
  arrowLeft: `<path d="M10.2 5.2L5.5 8.0l4.7 2.8V5.2Z" fill="currentColor"/>`,
  arrowRight: `<path d="M5.8 5.2L10.5 8.0l-4.7 2.8V5.2Z" fill="currentColor"/>`,
  arrowUp: `<path d="M5.2 10.2L8.0 5.5l2.8 4.7H5.2Z" fill="currentColor"/>`,
  arrowDown: `<path d="M5.2 5.8L8.0 10.5l2.8-4.7H5.2Z" fill="currentColor"/>`,
};

function buildModernMacBookLayout() {
  const width = 850;
  const height = 364;
  const padX = 14;
  const padY = 14;
  const gap = 6;
  const keyH = 51;
  const innerW = width - padX * 2; // 822
  const u = (innerW - 13 * gap) / 14.5; // 51.3103px per 1u key

  const keys = [];

  function pushRow(rowIdx, specs) {
    const y = padY + rowIdx * (keyH + gap);
    let x = padX;
    for (const s of specs) {
      const w = s.w;
      keys.push({ ...s, x, y, w, h: s.h ?? keyH });
      x += w + gap;
    }
  }

  // Row 0: Full-height modern MacBook function row (14 keys)
  const wEsc = innerW - 13 * u - 13 * gap; // 1.5u exact
  pushRow(0, [
    { code: "Escape", w: wEsc, label: "esc", corner: "bl", size: 10.5 },
    { code: "F1", w: u, glyph: "brightnessDown", sub: "F1" },
    { code: "F2", w: u, glyph: "brightnessUp", sub: "F2" },
    { code: "F3", w: u, glyph: "missionControl", sub: "F3" },
    { code: "F4", w: u, glyph: "spotlight", sub: "F4" },
    { code: "F5", w: u, glyph: "dictation", sub: "F5" },
    { code: "F6", w: u, glyph: "dnd", sub: "F6" },
    { code: "F7", w: u, glyph: "rewind", sub: "F7" },
    { code: "F8", w: u, glyph: "playPause", sub: "F8" },
    { code: "F9", w: u, glyph: "fastForward", sub: "F9" },
    { code: "F10", w: u, glyph: "mute", sub: "F10" },
    { code: "F11", w: u, glyph: "volumeDown", sub: "F11" },
    { code: "F12", w: u, glyph: "volumeUp", sub: "F12" },
    { code: "F13", w: u, glyphOnly: "lock", ariaLabel: "Touch ID / Lock" },
  ]);

  // Row 1: ANSI number row + delete (14 keys)
  const wDelete = innerW - 13 * u - 13 * gap;
  pushRow(1, [
    { code: "Backquote", w: u, top: "~", bottom: "`" },
    { code: "Digit1", w: u, top: "!", bottom: "1" },
    { code: "Digit2", w: u, top: "@", bottom: "2" },
    { code: "Digit3", w: u, top: "#", bottom: "3" },
    { code: "Digit4", w: u, top: "$", bottom: "4" },
    { code: "Digit5", w: u, top: "%", bottom: "5" },
    { code: "Digit6", w: u, top: "^", bottom: "6" },
    { code: "Digit7", w: u, top: "&", bottom: "7" },
    { code: "Digit8", w: u, top: "*", bottom: "8" },
    { code: "Digit9", w: u, top: "(", bottom: "9" },
    { code: "Digit0", w: u, top: ")", bottom: "0" },
    { code: "Minus", w: u, top: "_", bottom: "-" },
    { code: "Equal", w: u, top: "+", bottom: "=" },
    { code: "Backspace", w: wDelete, label: "delete", corner: "br", size: 10.5 },
  ]);

  // Row 2: tab + QWERTY + ANSI backslash (14 keys)
  const wTab = innerW - 13 * u - 13 * gap;
  pushRow(2, [
    { code: "Tab", w: wTab, label: "tab", corner: "bl", size: 10.5 },
    ..."QWERTYUIOP".split("").map((ch) => ({ code: `Key${ch}`, w: u, center: ch })),
    { code: "BracketLeft", w: u, top: "{", bottom: "[" },
    { code: "BracketRight", w: u, top: "}", bottom: "]" },
    { code: "Backslash", w: u, top: "|", bottom: "\\" },
  ]);

  // Row 3: caps lock + ASDF + horizontal ANSI return (13 keys)
  const wCapsReturn = (innerW - 11 * u - 12 * gap) / 2;
  pushRow(3, [
    { code: "CapsLock", w: wCapsReturn, label: "caps lock", corner: "bl", size: 10.5, capsDot: true },
    ..."ASDFGHJKL".split("").map((ch) => ({
      code: `Key${ch}`,
      w: u,
      center: ch,
      homing: ch === "F" || ch === "J",
    })),
    { code: "Semicolon", w: u, top: ":", bottom: ";" },
    { code: "Quote", w: u, top: '"', bottom: "'" },
    { code: "Enter", w: wCapsReturn, label: "return", corner: "br", size: 10.5 },
  ]);

  // Row 4: left shift + ZXCV + right shift (12 keys)
  const wShift = (innerW - 10 * u - 11 * gap) / 2;
  pushRow(4, [
    { code: "ShiftLeft", w: wShift, label: "shift", corner: "bl", size: 10.5 },
    ..."ZXCVBNM".split("").map((ch) => ({ code: `Key${ch}`, w: u, center: ch })),
    { code: "Comma", w: u, top: "<", bottom: "," },
    { code: "Period", w: u, top: ">", bottom: "." },
    { code: "Slash", w: u, top: "?", bottom: "/" },
    { code: "ShiftRight", w: wShift, label: "shift", corner: "br", size: 10.5 },
  ]);

  // Row 5: Globe, control, option, command, Space, command, option, Left, Up/Down split, Right (11 keys)
  const y5 = padY + 5 * (keyH + gap);
  const wCmd = u * 1.26;
  const wSpace = innerW - (7 * u + 2 * wCmd + 9 * gap);
  let x5 = padX;

  const row5Items = [
    { code: "Fn", w: u, cornerGlyph: { name: "globe", pos: "bl" }, ariaLabel: "Globe / Fn" },
    { code: "ControlLeft", w: u, modGlyph: "control", modGlyphSide: "left", modLabel: "control" },
    { code: "AltLeft", w: u, modGlyph: "option", modGlyphSide: "left", modLabel: "option" },
    { code: "MetaLeft", w: wCmd, modGlyph: "command", modGlyphSide: "left", modLabel: "command" },
    { code: "Space", w: wSpace, ariaLabel: "Space" },
    { code: "MetaRight", w: wCmd, modGlyph: "command", modGlyphSide: "right", modLabel: "command" },
    { code: "AltRight", w: u, modGlyph: "option", modGlyphSide: "right", modLabel: "option" },
    { code: "ArrowLeft", w: u, glyphOnly: "arrowLeft", glyphSize: 11, ariaLabel: "Arrow Left" },
  ];

  for (const item of row5Items) {
    keys.push({ ...item, x: x5, y: y5, h: keyH });
    x5 += item.w + gap;
  }

  // Split Up / Down arrow pair (two stacked half-height keys with crisp center divider)
  const splitGap = 2.5;
  const halfH = (keyH - splitGap) / 2;
  keys.push({
    code: "ArrowUp",
    x: x5,
    y: y5,
    w: u,
    h: halfH,
    splitHalf: "top",
    glyphOnly: "arrowUp",
    glyphSize: 10,
    ariaLabel: "Arrow Up",
  });
  keys.push({
    code: "ArrowDown",
    x: x5,
    y: y5 + halfH + splitGap,
    w: u,
    h: halfH,
    splitHalf: "bottom",
    glyphOnly: "arrowDown",
    glyphSize: 10,
    ariaLabel: "Arrow Down",
  });
  x5 += u + gap;

  keys.push({
    code: "ArrowRight",
    x: x5,
    y: y5,
    w: u,
    h: keyH,
    glyphOnly: "arrowRight",
    glyphSize: 11,
    ariaLabel: "Arrow Right",
  });

  return { width, height, keys };
}

function roundedRectPath(w, h, rTL, rTR, rBR, rBL) {
  return [
    `M ${rTL} 0`,
    `H ${w - rTR}`,
    `Q ${w} 0 ${w} ${rTR}`,
    `V ${h - rBR}`,
    `Q ${w} ${h} ${w - rBR} ${h}`,
    `H ${rBL}`,
    `Q 0 ${h} 0 ${h - rBL}`,
    `V ${rTL}`,
    `Q 0 0 ${rTL} 0`,
    "Z",
  ].join(" ");
}

function appendGlyph(parent, name, cx, cy, size = 14) {
  const markup = MAC_GLYPHS[name];
  if (!markup) return;
  const g = svgEl("svg", {
    class: "kb-key-glyph",
    x: (cx - size / 2).toFixed(2),
    y: (cy - size / 2).toFixed(2),
    width: size,
    height: size,
    viewBox: "0 0 16 16",
  });
  g.innerHTML = markup;
  parent.appendChild(g);
}

function appendText(parent, text, x, y, size, anchor = "middle", weight = 450) {
  const txt = svgEl("text", {
    class: "kb-key-legend",
    x: x.toFixed(2),
    y: (y + size * 0.34).toFixed(2),
    "font-size": size,
    "font-family": "var(--font-ui)",
    "font-weight": weight,
    "text-anchor": anchor,
  });
  txt.textContent = text;
  parent.appendChild(txt);
}

export async function mountInteractiveKeyboard(
  container,
  { onPointerAudition, onVisualActivity, onKeyTyped },
) {
  const layout = buildModernMacBookLayout();

  const svg = svgEl("svg", {
    class: "keyboard-svg",
    viewBox: `0 0 ${layout.width} ${layout.height}`,
    role: "group",
    "aria-label": "Interactive modern MacBook keyboard preview",
  });

  // Theme-adaptive chassis & keycap gradients (Light: Silver/White, Dark: Space Black)
  const defs = svgEl("defs");
  const chassisGrad = svgEl("linearGradient", {
    id: "kbChassisGrad",
    x1: "0%",
    y1: "0%",
    x2: "0%",
    y2: "100%",
  });
  chassisGrad.appendChild(svgEl("stop", { offset: "0%", class: "kb-stop-chassis-top" }));
  chassisGrad.appendChild(svgEl("stop", { offset: "100%", class: "kb-stop-chassis-bottom" }));
  defs.appendChild(chassisGrad);

  const capGrad = svgEl("linearGradient", {
    id: "kbCapGrad",
    x1: "0%",
    y1: "0%",
    x2: "0%",
    y2: "100%",
  });
  capGrad.appendChild(svgEl("stop", { offset: "0%", class: "kb-stop-cap-top" }));
  capGrad.appendChild(svgEl("stop", { offset: "100%", class: "kb-stop-cap-bottom" }));
  defs.appendChild(capGrad);
  svg.appendChild(defs);

  // Rounded aluminum chassis (silver in light mode, Space Black in dark mode)
  svg.appendChild(
    svgEl("rect", {
      class: "kb-chassis",
      x: 0.75,
      y: 0.75,
      width: layout.width - 1.5,
      height: layout.height - 1.5,
      rx: 22,
      fill: "url(#kbChassisGrad)",
      "stroke-width": 1.2,
    }),
  );

  // Subtle inner bevel highlight
  svg.appendChild(
    svgEl("rect", {
      class: "kb-chassis-bevel",
      x: 2,
      y: 2,
      width: layout.width - 4,
      height: layout.height - 4,
      rx: 20.5,
      fill: "none",
      "stroke-width": 1,
    }),
  );

  const keyNodes = new Map();
  const tracker = new KeyHoldTracker();

  function syncKeyVisual(code) {
    const node = keyNodes.get(code);
    if (!node) return;
    const pressed = tracker.isPressed(code);
    node.setAttribute("data-pressed", pressed ? "true" : "false");
    node.setAttribute("aria-pressed", pressed ? "true" : "false");
  }

  function clearAllVisuals() {
    const codes = tracker.clearAll();
    for (const code of codes) {
      syncKeyVisual(code);
    }
  }

  for (const key of layout.keys) {
    const r = 6.5;
    const pathD =
      key.splitHalf === "top"
        ? roundedRectPath(key.w, key.h, r, r, 2.5, 2.5)
        : key.splitHalf === "bottom"
          ? roundedRectPath(key.w, key.h, 2.5, 2.5, r, r)
          : roundedRectPath(key.w, key.h, r, r, r, r);

    const g = svgEl("g", {
      class: "kb-key",
      transform: `translate(${key.x.toFixed(2)}, ${key.y.toFixed(2)})`,
      "data-code": key.code,
      "data-pressed": "false",
      role: "button",
      tabindex: "-1",
      "aria-label": keyLabelForAccessibility(key),
      "aria-pressed": "false",
    });

    // Subtle key well shadow
    g.appendChild(
      svgEl("path", {
        class: "kb-key-well",
        d: pathD,
        transform: "translate(0, 1.5)",
      }),
    );

    const body = svgEl("g", { class: "kb-key-body" });
    body.appendChild(
      svgEl("path", {
        class: "kb-key-face",
        d: pathD,
      }),
    );

    // Subtle top & bottom keycap specular highlights (prominent on Space Black dark keys)
    const insetX = Math.min(6, key.w * 0.16);
    body.appendChild(
      svgEl("line", {
        class: "kb-key-hl-top",
        x1: insetX.toFixed(2),
        y1: "1.1",
        x2: (key.w - insetX).toFixed(2),
        y2: "1.1",
        "stroke-width": "0.95",
        "stroke-linecap": "round",
      }),
    );
    body.appendChild(
      svgEl("line", {
        class: "kb-key-hl-bottom",
        x1: insetX.toFixed(2),
        y1: (key.h - 1.1).toFixed(2),
        x2: (key.w - insetX).toFixed(2),
        y2: (key.h - 1.1).toFixed(2),
        "stroke-width": "0.95",
        "stroke-linecap": "round",
      }),
    );

    // Key legends & macOS glyphs
    if (key.center) {
      appendText(body, key.center, key.w / 2, key.h / 2, 14.5, "middle", 450);
    }
    if (key.homing) {
      body.appendChild(
        svgEl("rect", {
          class: "kb-key-marker",
          x: (key.w / 2 - 5.5).toFixed(2),
          y: (key.h - 10.5).toFixed(2),
          width: 11,
          height: 1.3,
          rx: 0.65,
        }),
      );
    }
    if (key.top !== undefined && key.bottom !== undefined) {
      appendText(body, key.top, key.w / 2, key.h * 0.31, 11.5, "middle", 450);
      appendText(body, key.bottom, key.w / 2, key.h * 0.69, 13, "middle", 450);
    }
    if (key.glyph && key.sub) {
      appendGlyph(body, key.glyph, key.w / 2, key.h * 0.42, 14);
      appendText(body, key.sub, key.w / 2, key.h * 0.78, 8.2, "middle", 500);
    }
    if (key.glyphOnly) {
      appendGlyph(body, key.glyphOnly, key.w / 2, key.h / 2, key.glyphSize ?? 14);
    }
    if (key.cornerGlyph) {
      appendGlyph(body, key.cornerGlyph.name, 14, key.h - 14, 14);
    }
    if (key.capsDot) {
      body.appendChild(
        svgEl("circle", {
          class: "kb-key-marker",
          cx: 11.5,
          cy: 11.5,
          r: 1.8,
        }),
      );
    }
    if (key.label) {
      const isRight = key.corner === "br";
      appendText(
        body,
        key.label,
        isRight ? key.w - 8.5 : 8.5,
        key.h - 11.5,
        key.size ?? 10.5,
        isRight ? "end" : "start",
        450,
      );
    }
    if (key.modLabel) {
      const gx = key.modGlyphSide === "right" ? key.w - 12 : 12;
      appendGlyph(body, key.modGlyph, gx, 13, 12.5);
      appendText(body, key.modLabel, key.w / 2, key.h - 11, 9.5, "middle", 450);
    }

    g.appendChild(body);

    // Pointer / touch audition handlers
    g.addEventListener("pointerdown", (ev) => {
      if (ev.pointerType === "mouse" && ev.button !== 0) return;
      ev.preventDefault();
      try {
        g.setPointerCapture(ev.pointerId);
      } catch {
        // Ignore if capture is unsupported in synthetic tests
      }
      const prev = tracker.pointerDown(ev.pointerId, key.code);
      if (prev) syncKeyVisual(prev);
      syncKeyVisual(key.code);
      onVisualActivity?.(key.code, "pointer");
      onPointerAudition?.(key.code, "down");

      if (onKeyTyped) {
        const keyRect = g.getBoundingClientRect();
        if (key.code === "Backspace") {
          onKeyTyped({ code: key.code, action: "backspace", keyRect });
        } else if (key.code === "Space") {
          onKeyTyped({ code: key.code, char: " ", action: "char", keyRect });
        } else if (key.center) {
          onKeyTyped({ code: key.code, char: key.center, action: "char", keyRect });
        } else if (key.bottom) {
          onKeyTyped({ code: key.code, char: key.bottom, action: "char", keyRect });
        }
      }
    });

    const releasePointer = (ev) => {
      const releasedCode = tracker.pointerUp(ev.pointerId);
      if (!releasedCode) return;
      syncKeyVisual(releasedCode);
      onPointerAudition?.(releasedCode, "up");
    };

    g.addEventListener("pointerup", releasePointer);
    g.addEventListener("pointercancel", releasePointer);
    g.addEventListener("lostpointercapture", releasePointer);

    keyNodes.set(key.code, g);
    svg.appendChild(g);
  }

  container.replaceChildren(svg);

  // Physical keyboard listeners (visual feedback ONLY; audio comes from daemon when enabled)
  function isEditableTarget(target) {
    if (!target || !(target instanceof HTMLElement)) return false;
    const tag = target.tagName;
    return tag === "INPUT" || tag === "TEXTAREA" || target.isContentEditable;
  }

  function handleKeyDown(ev) {
    if (ev.repeat) return;
    const code = ev.code;
    if (!code || !keyNodes.has(code)) return;

    const editable = isEditableTarget(ev.target);
    const changed = tracker.physicalDown(code);
    if (changed) {
      syncKeyVisual(code);
      if (!editable) {
        onVisualActivity?.(code, "physical");
      } else {
        onVisualActivity?.(code, "physical-input");
      }
    }

    if (!editable && onKeyTyped && !ev.ctrlKey && !ev.metaKey && !ev.altKey) {
      const node = keyNodes.get(code);
      const keyRect = node?.getBoundingClientRect?.() ?? null;
      if (ev.key === "Backspace") {
        onKeyTyped({ code, action: "backspace", keyRect });
      } else if (typeof ev.key === "string" && ev.key.length === 1) {
        onKeyTyped({ code, char: ev.key, action: "char", keyRect });
      }
    }
  }

  function handleKeyUp(ev) {
    const code = ev.code;
    if (!code || !keyNodes.has(code)) return;
    if (tracker.physicalUp(code)) {
      syncKeyVisual(code);
    }
  }

  function handleVisibilityOrBlur() {
    clearAllVisuals();
  }

  window.addEventListener("keydown", handleKeyDown, { passive: true });
  window.addEventListener("keyup", handleKeyUp, { passive: true });
  window.addEventListener("blur", handleVisibilityOrBlur);
  window.addEventListener("pagehide", handleVisibilityOrBlur);
  document.addEventListener("visibilitychange", () => {
    if (document.hidden) clearAllVisuals();
  });

  return {
    keyCount: layout.keys.length,
    clearHeld: clearAllVisuals,
    destroy() {
      window.removeEventListener("keydown", handleKeyDown);
      window.removeEventListener("keyup", handleKeyUp);
      window.removeEventListener("blur", handleVisibilityOrBlur);
      window.removeEventListener("pagehide", handleVisibilityOrBlur);
      clearAllVisuals();
    },
  };
}

function keyLabelForAccessibility(key) {
  if (key.ariaLabel) return key.ariaLabel;
  if (key.center) return key.center;
  if (key.label) return key.label;
  if (key.modLabel) return key.modLabel;
  if (key.sub) return key.sub;
  if (key.bottom) return `${key.bottom} ${key.top || ""}`.trim();
  return key.code;
}
