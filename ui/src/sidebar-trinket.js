/**
 * Thocci sidebar trinket controller & persistent SVG rig.
 *
 * Implements the approved three-keycap side-profile animation anchored at the
 * bottom of the Settings sidebar:
 * - Reuses the exact Ink silhouette & eye geometry from `thocci-quick-actions.svg`
 * - Renders three blank side-profile keycaps with concealed stems against one deck
 * - Hops between keycaps on dashboard keyboard activity (`onVisualActivity`),
 *   accelerating up to ~2.45x during sustained bursts and resetting cadence after ~720 ms
 * - Depresses & highlights the landing keycap using the digital keyboard's CSS tokens
 *   (`--kb-pressed`, `--kb-pressed-rim`, `--kb-pressed-glow`)
 * - Honors `Animate Thocci` off, `prefers-reduced-motion`, dashboard/pane suspension,
 *   and `document.hidden`, running zero rAF frames while idle or suspended.
 */

import { loadTextAsset } from "./icons.js";

const SVG_NS = "http://www.w3.org/2000/svg";

export const TRINKET_KEY_POSITIONS = Object.freeze([34, 96, 158]);
export const TRINKET_PERCH_Y = 93;
export const TRINKET_SWITCH_TRAVEL = 10.5;
export const TRINKET_CADENCE_RESET_MS = 720;
export const TRINKET_PRESS_DURATION_MS = 218;
export const TRINKET_LAUNCH_MS = 44;
export const TRINKET_FLIGHT_SHORT_MS = 202;
export const TRINKET_FLIGHT_WIDE_MS = 245;
export const TRINKET_REDUCED_RECOVERY_MS = 145;

const CAT_BASE_SCALE = 0.285;
const FALLBACK_BODY_PATH =
  "M14 106C5 100 0 90 0 79c0-12 5-24 13-35l13-27c3-7 11-8 15-2l9 12c12-6 26-10 43-10l19-14c6-5 13-3 16 3l24 43c5 10 8 20 8 31 0 13-6 24-17 29-17 7-39 10-63 10-26 0-51-3-66-13Z";
const FALLBACK_EYES = Object.freeze([
  { x: "76", y: "49", width: "11", height: "20", rx: "5.5" },
  { x: "97", y: "48", width: "11", height: "20", rx: "5.5" },
]);

const clamp01 = (value) => Math.min(1, Math.max(0, value));

let mascotArtPromise = null;

async function loadTrinketMascotArt() {
  if (!mascotArtPromise) {
    mascotArtPromise = loadTextAsset("./vendor/mascot/thocci-quick-actions.svg")
      .then((svgText) => {
        if (typeof DOMParser !== "function") {
          return { bodyPath: FALLBACK_BODY_PATH, eyes: FALLBACK_EYES };
        }
        const doc = new DOMParser().parseFromString(svgText, "image/svg+xml");
        const bodyPath =
          doc.querySelector("#thocci-body path")?.getAttribute("d") || FALLBACK_BODY_PATH;
        const eyeRects = Array.from(doc.querySelectorAll("#thocci-eyes rect")).map((rect) => ({
          x: rect.getAttribute("x") || "76",
          y: rect.getAttribute("y") || "49",
          width: rect.getAttribute("width") || "11",
          height: rect.getAttribute("height") || "20",
          rx: rect.getAttribute("rx") || "5.5",
        }));
        return {
          bodyPath,
          eyes: eyeRects.length >= 2 ? eyeRects : FALLBACK_EYES,
        };
      })
      .catch(() => ({ bodyPath: FALLBACK_BODY_PATH, eyes: FALLBACK_EYES }));
  }
  return mascotArtPromise;
}

/**
 * Pure, DOM-free sidebar trinket state machine & pose calculator.
 * Exported for deterministic Node unit testing alongside `createMascotController`.
 */
export function createSidebarTrinketController({
  initialAnimated = true,
  initialReducedMotion = false,
  random = Math.random,
} = {}) {
  let animated = Boolean(initialAnimated);
  let reduced = Boolean(initialReducedMotion);
  let suspended = false;

  let currentKey = 1; // Middle keycap at x=96
  let activeHop = null;
  let pendingHop = false;
  let lastInputAt = -Infinity;
  let typingInterval = Infinity;

  function hopSpeed(nowMs) {
    const cadence = clamp01((430 - typingInterval) / 335);
    const recency = 1 - clamp01((nowMs - lastInputAt - 340) / (TRINKET_CADENCE_RESET_MS - 340));
    return 1 + 1.45 * cadence * recency;
  }

  function startHop(nowMs) {
    const offset = random() < 0.5 ? 1 : 2;
    const nextKey = (currentKey + offset) % TRINKET_KEY_POSITIONS.length;
    const from = TRINKET_KEY_POSITIONS[currentKey];
    const to = TRINKET_KEY_POSITIONS[nextKey];
    const distance = to - from;
    const acrossTwoKeys = Math.abs(distance) > 70;
    const still = reduced;
    const launch = still ? 0 : TRINKET_LAUNCH_MS;
    const flight = still
      ? 0
      : acrossTwoKeys
        ? TRINKET_FLIGHT_WIDE_MS
        : TRINKET_FLIGHT_SHORT_MS;
    const recovery = still ? TRINKET_REDUCED_RECOVERY_MS : TRINKET_PRESS_DURATION_MS;

    activeHop = {
      fromKey: currentKey,
      nextKey,
      from,
      to,
      distance,
      height: acrossTwoKeys ? 36 : 26,
      still,
      launch,
      flight,
      landing: launch + flight,
      recovery,
      elapsed: 0,
      lastFrame: nowMs,
    };
  }

  function buildRestingFrame(expression = "open") {
    const x = TRINKET_KEY_POSITIONS[currentKey];
    return {
      hopping: false,
      currentKey,
      activeKeyIndex: -1,
      keys: TRINKET_KEY_POSITIONS.map((kx) => ({
        x: kx,
        travel: 0,
        lightOpacity: 0,
      })),
      glow: { x, opacity: 0 },
      impact: { x, y: TRINKET_PERCH_Y + 1, drift: 0, opacity: 0 },
      shadow: { cx: x, cy: TRINKET_PERCH_Y, rx: 17, opacity: 0.55 },
      cat: {
        x,
        y: TRINKET_PERCH_Y,
        stretch: 1,
        squash: 1,
        tilt: 0,
      },
      expression,
    };
  }

  function clearMotionAndCadence() {
    activeHop = null;
    pendingHop = false;
    lastInputAt = -Infinity;
    typingInterval = Infinity;
  }

  function triggerKeystroke(nowMs) {
    if (!animated || suspended) return false;

    const gap = nowMs - lastInputAt;
    if (!Number.isFinite(gap) || gap >= TRINKET_CADENCE_RESET_MS) {
      typingInterval = Infinity;
    } else if (gap >= 0) {
      typingInterval = Number.isFinite(typingInterval)
        ? 0.3 * typingInterval + 0.7 * gap
        : gap;
    }
    lastInputAt = nowMs;

    if (activeHop) {
      pendingHop = true;
      return true;
    }

    startHop(nowMs);
    return true;
  }

  function step(nowMs) {
    if (!activeHop) {
      return buildRestingFrame("open");
    }

    const hop = activeHop;
    const dt = Math.max(0, nowMs - hop.lastFrame);
    hop.lastFrame = Math.max(hop.lastFrame, nowMs);

    const speed = hopSpeed(nowMs);
    const effectiveSpeed = hop.still ? Math.min(speed, 1.55) : speed;
    hop.elapsed += dt * effectiveSpeed;

    const total = hop.landing + hop.recovery;
    if (hop.elapsed >= total) {
      currentKey = hop.nextKey;
      activeHop = null;
      if (pendingHop && animated && !suspended) {
        pendingHop = false;
        startHop(nowMs);
        return renderCurrentHopFrame();
      }
      pendingHop = false;
      return buildRestingFrame("open");
    }

    return renderCurrentHopFrame();
  }

  function renderCurrentHopFrame() {
    if (!activeHop) {
      return buildRestingFrame("open");
    }

    const hop = activeHop;
    const keyStates = TRINKET_KEY_POSITIONS.map((kx) => ({
      x: kx,
      travel: 0,
      lightOpacity: 0,
    }));

    if (hop.elapsed < hop.launch) {
      const p = hop.launch > 0 ? hop.elapsed / hop.launch : 1;
      const bend = Math.sin(p * Math.PI);
      return {
        hopping: true,
        currentKey,
        activeKeyIndex: -1,
        keys: keyStates,
        glow: { x: hop.from, opacity: 0 },
        impact: { x: hop.from, y: TRINKET_PERCH_Y + 1, drift: 0, opacity: 0 },
        shadow: { cx: hop.from, cy: TRINKET_PERCH_Y, rx: 17, opacity: 0.55 },
        cat: {
          x: hop.from,
          y: TRINKET_PERCH_Y,
          stretch: 1 + bend * 0.085,
          squash: 1 - bend * 0.17,
          tilt: 0,
        },
        expression: "open",
      };
    }

    if (hop.elapsed < hop.landing) {
      const p = hop.flight > 0 ? (hop.elapsed - hop.launch) / hop.flight : 1;
      const x = hop.from + hop.distance * p;
      const lift = 4 * hop.height * p * (1 - p);
      return {
        hopping: true,
        currentKey,
        activeKeyIndex: -1,
        keys: keyStates,
        glow: { x: hop.from, opacity: 0 },
        impact: { x, y: TRINKET_PERCH_Y + 1, drift: 0, opacity: 0 },
        shadow: {
          cx: x,
          cy: TRINKET_PERCH_Y,
          rx: 17 - lift * 0.15,
          opacity: Math.max(0.04, 0.55 - lift * 0.014),
        },
        cat: {
          x,
          y: TRINKET_PERCH_Y - lift,
          stretch: 0.98,
          squash: 1 + 0.13 * Math.sin(p * Math.PI),
          tilt: Math.sign(hop.distance) * 12 * Math.sin(2 * Math.PI * p),
        },
        expression: "open",
      };
    }

    const age = clamp01((hop.elapsed - hop.landing) / hop.recovery) * TRINKET_PRESS_DURATION_MS;
    const down = clamp01(age / 40);
    const up = clamp01((age - 86) / 98);
    const travel =
      TRINKET_SWITCH_TRAVEL * (1 - (1 - down) ** 2) * (0.5 + 0.5 * Math.cos(up * Math.PI));
    const highlight = 1 - clamp01((age - 86) / 132);
    const lightOpacity = highlight * (0.74 + (0.26 * travel) / TRINKET_SWITCH_TRAVEL);
    const glowOpacity = highlight * (0.28 + (0.56 * travel) / TRINKET_SWITCH_TRAVEL);

    keyStates[hop.nextKey].travel = travel;
    keyStates[hop.nextKey].lightOpacity = lightOpacity;

    const spread = clamp01(age / 145);
    const drift = 9 * (1 - (1 - spread) ** 2);
    const impactOpacity = hop.still ? 0 : (1 - spread) ** 1.4;

    return {
      hopping: true,
      currentKey,
      activeKeyIndex: hop.nextKey,
      keys: keyStates,
      glow: { x: hop.to, opacity: glowOpacity },
      impact: {
        x: hop.to,
        y: TRINKET_PERCH_Y + 1 + travel * 0.22 - drift * 0.65,
        drift: hop.still ? 0 : drift,
        opacity: impactOpacity,
      },
      shadow: {
        cx: hop.to,
        cy: TRINKET_PERCH_Y + travel,
        rx: 17,
        opacity: 0.55,
      },
      cat: {
        x: hop.to,
        y: TRINKET_PERCH_Y + travel,
        stretch: hop.still ? 1 : 1 + travel * 0.014,
        squash: hop.still ? 1 : 1 - travel * 0.022,
        tilt: 0,
      },
      expression: age < 105 ? "land" : "open",
    };
  }

  function setAnimated(nextAnimated) {
    const next = Boolean(nextAnimated);
    if (next === animated) return false;
    animated = next;
    if (!animated) {
      clearMotionAndCadence();
      currentKey = 1;
    }
    return true;
  }

  function setReducedMotion(nextReduced) {
    const next = Boolean(nextReduced);
    if (next === reduced) return false;
    reduced = next;
    return true;
  }

  function setSuspended(nextSuspended) {
    const next = Boolean(nextSuspended);
    if (next === suspended) return false;
    suspended = next;
    if (suspended) {
      clearMotionAndCadence();
    }
    return true;
  }

  function getState(nowMs = lastInputAt) {
    return {
      animated,
      reduced,
      suspended,
      currentKey,
      hopping: Boolean(activeHop),
      pendingHop,
      typingInterval,
      speed: Number.isFinite(nowMs) ? hopSpeed(nowMs) : 1,
    };
  }

  return {
    triggerKeystroke,
    step,
    buildRestingFrame,
    clearMotionAndCadence,
    setAnimated,
    setReducedMotion,
    setSuspended,
    getState,
    hopSpeed,
  };
}

function buildTrinketSvgMarkup({ bodyPath, eyes }) {
  const eyeRects = eyes
    .map(
      (e) =>
        `<rect x="${e.x}" y="${e.y}" width="${e.width}" height="${e.height}" rx="${e.rx}"/>`,
    )
    .join("");

  return `
    <svg class="sidebar-trinket-svg" viewBox="0 0 192 142" aria-hidden="true" focusable="false">
      <defs>
        <linearGradient id="trinket-cap-side" x1=".18" y1="0" x2=".44" y2="1">
          <stop stop-color="var(--trinket-cap-highlight)"/>
          <stop offset=".28" stop-color="var(--trinket-cap-surface)"/>
          <stop offset=".78" stop-color="var(--trinket-cap-side)"/>
          <stop offset="1" stop-color="var(--trinket-cap-base)"/>
        </linearGradient>
        <radialGradient id="trinket-impact-halo">
          <stop stop-color="var(--kb-pressed-rim)" stop-opacity=".65"/>
          <stop offset="1" stop-color="var(--kb-pressed-rim)" stop-opacity="0"/>
        </radialGradient>
        <linearGradient id="trinket-deck-side" x2="0" y2="1">
          <stop stop-color="var(--kb-chassis-top)"/>
          <stop offset="1" stop-color="var(--kb-chassis-bottom)"/>
        </linearGradient>
        <clipPath id="trinket-deck-clip" clipPathUnits="userSpaceOnUse">
          <rect x="-30" y="60" width="60" height="61"/>
        </clipPath>
        <path id="trinket-cap-profile" d="M-17.8 88.5C-10.6 88.5-4.5 92.5 4 92.8C9.1 93 13.7 92.1 17.2 91Q20.5 89.9 21.4 93.5L27.3 113.9Q28.3 117.8 24.6 117.8H-24.7Q-28.2 117.8-27.4 114L-22.2 93.3Q-21.1 88.5-17.8 88.5Z"/>
        <path id="trinket-cap-ridge" d="M-17.5 88.9C-10.6 89-4.4 92.9 4 93.2C9.1 93.4 13.7 92.4 17.3 91.4Q19.8 90.6 21 93.8" fill="none" stroke-linecap="round"/>
        <g id="trinket-cap-art">
          <use href="#trinket-cap-profile" fill="url(#trinket-cap-side)" stroke="var(--trinket-cap-edge)" stroke-opacity=".55" stroke-width=".75"/>
          <use href="#trinket-cap-ridge" stroke="var(--trinket-cap-highlight)" stroke-opacity=".78" stroke-width=".8"/>
          <path d="M-21.6 95.7-26.1 114.1M-23.7 116.5H24" fill="none" stroke="var(--trinket-cap-edge)" stroke-opacity=".18" stroke-width=".75" stroke-linecap="round"/>
        </g>
        <g id="trinket-lit-cap">
          <use href="#trinket-cap-profile" fill="var(--kb-pressed)" stroke="var(--kb-pressed-rim)" stroke-width=".75"/>
          <use href="#trinket-cap-ridge" stroke="var(--kb-pressed-rim)" stroke-width="1.1"/>
        </g>
      </defs>
      <ellipse cx="96" cy="132" rx="89" ry="4" fill="var(--trinket-deck-shadow)"/>
      <path d="M5 120H187V125H5Z" fill="var(--kb-key-well)"/>
      <ellipse data-trinket-glow cx="96" cy="121" rx="32" ry="8" fill="url(#trinket-impact-halo)" opacity="0"/>
      <g data-trinket-switch="34" transform="translate(34 0)" clip-path="url(#trinket-deck-clip)">
        <g data-trinket-key transform="translate(0 0)">
          <use href="#trinket-cap-art"/>
          <use data-trinket-key-light href="#trinket-lit-cap" opacity="0"/>
        </g>
      </g>
      <g data-trinket-switch="96" transform="translate(96 0)" clip-path="url(#trinket-deck-clip)">
        <g data-trinket-key transform="translate(0 0)">
          <use href="#trinket-cap-art"/>
          <use data-trinket-key-light href="#trinket-lit-cap" opacity="0"/>
        </g>
      </g>
      <g data-trinket-switch="158" transform="translate(158 0)" clip-path="url(#trinket-deck-clip)">
        <g data-trinket-key transform="translate(0 0)">
          <use href="#trinket-cap-art"/>
          <use data-trinket-key-light href="#trinket-lit-cap" opacity="0"/>
        </g>
      </g>
      <path d="M5 121H187V127Q187 131 183 131H9Q5 131 5 127Z" fill="url(#trinket-deck-side)" stroke="var(--kb-chassis-rim)" stroke-width=".7"/>
      <path d="M8.5 121.5H183.5" stroke="var(--kb-chassis-bevel)" stroke-width=".85" stroke-linecap="round"/>
      <g data-trinket-impact aria-hidden="true" opacity="0">
        <g data-trinket-hit-left fill="none" stroke="var(--kb-pressed-rim)" stroke-width="1.55" stroke-linecap="round">
          <path d="M-22-4l-5-3m4 1-2-5"/>
          <circle cx="-28" cy="-1" r=".8" fill="var(--kb-pressed-rim)" stroke="none"/>
        </g>
        <g data-trinket-hit-right fill="none" stroke="var(--kb-pressed-rim)" stroke-width="1.55" stroke-linecap="round">
          <path d="M22-4l5-3m-4 1 2-5"/>
          <circle cx="28" cy="-1" r=".8" fill="var(--kb-pressed-rim)" stroke="none"/>
        </g>
      </g>
      <ellipse data-trinket-shadow cx="96" cy="93" rx="17" ry="3" fill="var(--trinket-contact-shadow)" opacity=".55"/>
      <g data-trinket-cat transform="translate(96 93) scale(.285) translate(-80 -119)">
        <path d="${bodyPath}" fill="#17181b" stroke="#606069" stroke-width="2.4"/>
        <g data-trinket-face data-expression="open">
          <g class="trinket-eyes-open" fill="#fdfdff">${eyeRects}</g>
          <path class="trinket-eyes-squint" d="m76 63 6-6 6 6m9 0 6-6 6 6" fill="none" stroke="#fdfdff" stroke-width="3.8" stroke-linecap="round" stroke-linejoin="round"/>
          <path class="trinket-eyes-closed" d="M78 56c3 5 7 5 10 0m10 0c3 5 7 5 10 0" fill="none" stroke="#fdfdff" stroke-width="3.8" stroke-linecap="round"/>
        </g>
      </g>
    </svg>
  `;
}

/**
 * Mount the persistent sidebar trinket SVG and bind one on-demand rAF loop.
 */
export async function mountSidebarTrinket(container, { initialAnimated = true } = {}) {
  const art = await loadTrinketMascotArt();
  const reducedMotionQuery = window.matchMedia("(prefers-reduced-motion: reduce)");

  const controller = createSidebarTrinketController({
    initialAnimated,
    initialReducedMotion: reducedMotionQuery.matches,
  });

  container.innerHTML = buildTrinketSvgMarkup(art);
  const svg = container.querySelector("svg");
  const cat = svg.querySelector("[data-trinket-cat]");
  const face = cat.querySelector("[data-trinket-face]");
  const shadow = svg.querySelector("[data-trinket-shadow]");
  const glow = svg.querySelector("[data-trinket-glow]");
  const impact = svg.querySelector("[data-trinket-impact]");
  const leftMarks = impact.querySelector("[data-trinket-hit-left]");
  const rightMarks = impact.querySelector("[data-trinket-hit-right]");
  const keyNodes = Array.from(svg.querySelectorAll("[data-trinket-switch]"), (socket) => ({
    cap: socket.querySelector("[data-trinket-key]"),
    light: socket.querySelector("[data-trinket-key-light]"),
  }));

  let rafId = 0;
  let blinkTimer = 0;
  let destroyed = false;

  function clearBlink() {
    if (blinkTimer) {
      clearTimeout(blinkTimer);
      blinkTimer = 0;
    }
  }

  function stopLoop() {
    if (rafId) {
      cancelAnimationFrame(rafId);
      rafId = 0;
    }
    clearBlink();
  }

  function applyFrame(frame) {
    for (let i = 0; i < keyNodes.length; i++) {
      const k = frame.keys[i];
      keyNodes[i].cap.setAttribute("transform", `translate(0 ${k.travel.toFixed(3)})`);
      keyNodes[i].light.setAttribute("opacity", k.lightOpacity.toFixed(3));
    }

    glow.setAttribute("cx", frame.glow.x.toFixed(2));
    glow.setAttribute("opacity", frame.glow.opacity.toFixed(3));

    impact.setAttribute("opacity", frame.impact.opacity.toFixed(3));
    if (frame.impact.opacity > 0.001) {
      impact.setAttribute(
        "transform",
        `translate(${frame.impact.x.toFixed(2)} ${frame.impact.y.toFixed(2)})`,
      );
      leftMarks.setAttribute("transform", `translate(${(-frame.impact.drift).toFixed(2)} 0)`);
      rightMarks.setAttribute("transform", `translate(${frame.impact.drift.toFixed(2)} 0)`);
    }

    shadow.setAttribute("cx", frame.shadow.cx.toFixed(2));
    shadow.setAttribute("cy", frame.shadow.cy.toFixed(2));
    shadow.setAttribute("rx", frame.shadow.rx.toFixed(2));
    shadow.setAttribute("opacity", frame.shadow.opacity.toFixed(3));

    const sx = (CAT_BASE_SCALE * frame.cat.stretch).toFixed(4);
    const sy = (CAT_BASE_SCALE * frame.cat.squash).toFixed(4);
    cat.setAttribute(
      "transform",
      `translate(${frame.cat.x.toFixed(2)} ${frame.cat.y.toFixed(2)}) rotate(${frame.cat.tilt.toFixed(2)}) scale(${sx} ${sy}) translate(-80 -119)`,
    );

    if (face.dataset.expression !== frame.expression) {
      face.dataset.expression = frame.expression;
    }
  }

  function scheduleBlink() {
    clearBlink();
    const snap = controller.getState();
    if (destroyed || !snap.animated || snap.suspended || snap.reduced || snap.hopping || document.hidden) {
      return;
    }
    blinkTimer = setTimeout(() => {
      blinkTimer = 0;
      const cur = controller.getState();
      if (destroyed || !cur.animated || cur.suspended || cur.reduced || cur.hopping || document.hidden) {
        return;
      }
      face.dataset.expression = "blink";
      blinkTimer = setTimeout(() => {
        blinkTimer = 0;
        if (destroyed) return;
        const after = controller.getState();
        if (!after.hopping) {
          face.dataset.expression = "open";
          scheduleBlink();
        }
      }, 125);
    }, 2500 + Math.random() * 2100);
  }

  function tick(now) {
    rafId = 0;
    if (destroyed || document.hidden) {
      return;
    }
    const snap = controller.getState();
    if (!snap.animated || snap.suspended) {
      applyFrame(controller.buildRestingFrame("open"));
      return;
    }

    const frame = controller.step(now);
    applyFrame(frame);

    if (frame.hopping) {
      rafId = requestAnimationFrame(tick);
    } else {
      scheduleBlink();
    }
  }

  function ensureLoopRunning() {
    if (destroyed || rafId || document.hidden) return;
    rafId = requestAnimationFrame(tick);
  }

  function reactToKeystroke() {
    if (destroyed || document.hidden) return;
    const accepted = controller.triggerKeystroke(performance.now());
    if (!accepted) return;
    clearBlink();
    ensureLoopRunning();
  }

  function setAnimated(nextAnimated) {
    const changed = controller.setAnimated(nextAnimated);
    if (!changed) return;
    const snap = controller.getState();
    if (!snap.animated) {
      stopLoop();
      applyFrame(controller.buildRestingFrame("open"));
    } else {
      applyFrame(controller.buildRestingFrame("open"));
      scheduleBlink();
    }
  }

  function setSuspended(nextSuspended) {
    const changed = controller.setSuspended(nextSuspended);
    if (!changed) return;
    const snap = controller.getState();
    if (snap.suspended) {
      stopLoop();
      applyFrame(controller.buildRestingFrame("open"));
    } else {
      applyFrame(controller.buildRestingFrame("open"));
      scheduleBlink();
    }
  }

  function onVisibilityChange() {
    if (document.hidden) {
      stopLoop();
      controller.clearMotionAndCadence();
      applyFrame(controller.buildRestingFrame("open"));
    } else {
      scheduleBlink();
    }
  }

  function onReducedMotionChange() {
    const changed = controller.setReducedMotion(reducedMotionQuery.matches);
    if (!changed) return;
    if (reducedMotionQuery.matches) {
      clearBlink();
      if (!controller.getState().hopping) {
        face.dataset.expression = "open";
      }
    } else {
      scheduleBlink();
    }
  }

  document.addEventListener("visibilitychange", onVisibilityChange);
  reducedMotionQuery.addEventListener("change", onReducedMotionChange);

  applyFrame(controller.buildRestingFrame("open"));
  scheduleBlink();

  return {
    reactToKeystroke,
    setAnimated,
    setSuspended,
    getState: () => controller.getState(),
    destroy() {
      if (destroyed) return;
      destroyed = true;
      stopLoop();
      controller.clearMotionAndCadence();
      document.removeEventListener("visibilitychange", onVisibilityChange);
      reducedMotionQuery.removeEventListener("change", onReducedMotionChange);
    },
  };
}

export { SVG_NS };
