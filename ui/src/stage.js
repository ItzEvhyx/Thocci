/**
 * Thocci animated mascot stage (850 x 304 logical coordinate space).
 *
 * Uses `vendor/mascot/stage-poses.json` and bundled mascot/effect SVGs.
 * Supports:
 * - Peek up (initial resting pose), Peek from side, Slide, Jump, Sleep
 * - Coalesced typing reactions that vary naturally without jitter
 * - Idle sleep timer and immediate wake on next interaction
 * - Pause when hidden, suspended, or when `Animate Thocci` is off
 * - Reduced-motion fallback (still pose with gentle expression feedback)
 */

import { loadJsonAsset, loadTextAsset } from "./icons.js";

const SVG_NS = "http://www.w3.org/2000/svg";
const IDLE_SLEEP_MS = 12_000;
const RETURN_TO_REST_MS = 1_900;
const REACTION_COALESCE_MS = 420;

const REACTION_SEQUENCE = ["Jump", "Slide", "Peek from side", "Jump"];

function svgEl(tag, attrs = {}) {
  const el = document.createElementNS(SVG_NS, tag);
  for (const [k, v] of Object.entries(attrs)) {
    el.setAttribute(k, String(v));
  }
  return el;
}

export async function mountMascotStage(container, { initialTheme = "light", initialAnimated = true }) {
  const poseSpec = await loadJsonAsset("./vendor/mascot/stage-poses.json");
  const [openSvg, excitedSvg, sleepSvg, trailSvg, shadowSvg, sleepMarksSvg] =
    await Promise.all([
      loadTextAsset("./vendor/mascot/thocci-quick-actions.svg"),
      loadTextAsset("./vendor/mascot/thocci-excited.svg"),
      loadTextAsset("./vendor/mascot/thocci-sleeping.svg"),
      loadTextAsset("./vendor/mascot/slide-trail.svg"),
      loadTextAsset("./vendor/mascot/jump-shadow.svg"),
      loadTextAsset("./vendor/mascot/sleep-marks.svg"),
    ]);

  const assetMap = {
    "thocci-quick-actions.svg": openSvg,
    "thocci-excited.svg": excitedSvg,
    "thocci-sleeping.svg": sleepSvg,
    "slide-trail.svg": trailSvg,
    "jump-shadow.svg": shadowSvg,
    "sleep-marks.svg": sleepMarksSvg,
  };

  const posesByName = new Map(poseSpec.poses.map((p) => [p.name, p]));
  const { width, height, cornerRadius } = poseSpec.stage;
  const mascotW = poseSpec.mascot.width;
  const mascotH = poseSpec.mascot.height;

  const svg = svgEl("svg", {
    class: "stage-svg",
    viewBox: `0 0 ${width} ${height}`,
    role: "img",
    "aria-label": "Thocci mascot stage",
  });

  const defs = svgEl("defs");
  const grad = svgEl("linearGradient", {
    id: "thocci-stage-bg-grad",
    x1: "0%",
    x2: "0%",
    y1: "0%",
    y2: "100%",
  });
  const stopTop = svgEl("stop", { offset: "0%", "stop-color": "var(--stage-top)" });
  const stopBottom = svgEl("stop", { offset: "100%", "stop-color": "var(--stage-bottom)" });
  grad.append(stopTop, stopBottom);

  const clip = svgEl("clipPath", { id: "thocci-stage-clip" });
  clip.appendChild(svgEl("rect", { width, height, rx: cornerRadius }));
  defs.append(grad, clip);
  svg.appendChild(defs);

  const clippedGroup = svgEl("g", { "clip-path": "url(#thocci-stage-clip)" });

  // Stage backdrop & sound ripples (tinted automatically by theme CSS tokens)
  clippedGroup.appendChild(
    svgEl("rect", {
      x: 0.5,
      y: 0.5,
      width: width - 1,
      height: height - 1,
      rx: cornerRadius,
      fill: "url(#thocci-stage-bg-grad)",
      stroke: "var(--border-rim)",
      "stroke-width": 1,
    }),
  );

  const ripples = svgEl("g", { fill: "none" });
  ripples.append(
    svgEl("ellipse", {
      cx: 425,
      cy: 300,
      rx: 213,
      ry: 79,
      stroke: "var(--stage-orbit)",
      "stroke-opacity": "0.3",
    }),
    svgEl("ellipse", {
      cx: 425,
      cy: 301,
      rx: 293,
      ry: 113,
      stroke: "var(--stage-orbit)",
      "stroke-opacity": "0.22",
    }),
    svgEl("ellipse", {
      cx: 425,
      cy: 303,
      rx: 369,
      ry: 148,
      stroke: "var(--stage-orbit)",
      "stroke-opacity": "0.13",
    }),
    svgEl("path", {
      d: "M298 234c-6 5-9 12-9 19m263-19c6 5 9 12 9 19",
      stroke: "var(--stage-orbit)",
      "stroke-opacity": "0.58",
      "stroke-width": "1.5",
      "stroke-linecap": "round",
    }),
    svgEl("circle", {
      cx: 220,
      cy: 231,
      r: 2,
      fill: "var(--stage-orbit)",
      "fill-opacity": "0.55",
    }),
    svgEl("circle", {
      cx: 636,
      cy: 225,
      r: 2.5,
      fill: "var(--stage-orbit)",
      "fill-opacity": "0.5",
    }),
  );
  clippedGroup.appendChild(ripples);

  const bgEffectsLayer = svgEl("g", { class: "stage-effects-bg" });
  const mascotLayer = svgEl("g", { class: "stage-mascot-layer" });
  const fgEffectsLayer = svgEl("g", { class: "stage-effects-fg" });

  clippedGroup.append(bgEffectsLayer, mascotLayer, fgEffectsLayer);
  svg.appendChild(clippedGroup);
  container.replaceChildren(svg);

  const parser = new DOMParser();

  function parseInnerSvg(raw, targetW, targetH, extraStyleColor = null) {
    const doc = parser.parseFromString(raw, "image/svg+xml");
    const root = doc.documentElement;
    const node = svgEl("svg", {
      width: targetW,
      height: targetH,
      viewBox: root.getAttribute("viewBox") || `0 0 ${targetW} ${targetH}`,
    });
    if (extraStyleColor) {
      node.style.color = extraStyleColor;
    }
    for (const child of Array.from(root.childNodes)) {
      if (child.nodeType === 1) {
        node.appendChild(document.importNode(child, true));
      }
    }
    return node;
  }

  let animated = initialAnimated;
  let currentPoseName = poseSpec.initialPose || "Peek up";
  let reactionIndex = 0;
  let lastReactionTime = 0;
  let restTimer = null;
  let sleepTimer = null;

  const reducedMotionQuery = window.matchMedia("(prefers-reduced-motion: reduce)");

  function isReducedMotion() {
    return reducedMotionQuery.matches;
  }

  function updateMotionAttrs() {
    container.setAttribute("data-animated", animated ? "true" : "false");
    container.setAttribute("data-reduced-motion", isReducedMotion() ? "true" : "false");
    container.setAttribute("data-theme", initialTheme);
  }

  function clearTimers() {
    if (restTimer) {
      clearTimeout(restTimer);
      restTimer = null;
    }
    if (sleepTimer) {
      clearTimeout(sleepTimer);
      sleepTimer = null;
    }
  }

  function renderPose(poseName, overrideExpressionAsset = null) {
    const pose = posesByName.get(poseName) || posesByName.get("Peek up");
    if (!pose) return;
    currentPoseName = pose.name;
    container.setAttribute("data-pose", pose.name);

    const x = isReducedMotion() ? posesByName.get("Peek up").x : pose.x;
    const y = isReducedMotion() ? posesByName.get("Peek up").y : pose.y;
    const assetKey = overrideExpressionAsset || pose.asset;

    mascotLayer.style.transform = `translate(${x}px, ${y}px)`;
    mascotLayer.replaceChildren(parseInnerSvg(assetMap[assetKey], mascotW, mascotH));

    bgEffectsLayer.replaceChildren();
    fgEffectsLayer.replaceChildren();

    if (!isReducedMotion()) {
      for (const eff of pose.effects || []) {
        const raw = assetMap[eff.asset];
        if (!raw) continue;
        const color =
          eff.asset === "jump-shadow.svg" ? "#17181B" : "var(--stage-orbit)";
        const effNode = parseInnerSvg(raw, eff.width, eff.height, color);
        effNode.setAttribute("x", String(eff.x));
        effNode.setAttribute("y", String(eff.y));
        effNode.setAttribute("class", "stage-effect");
        if (eff.layer === "foreground") {
          fgEffectsLayer.appendChild(effNode);
        } else {
          bgEffectsLayer.appendChild(effNode);
        }
      }
    }
  }

  function scheduleIdleSleep() {
    if (sleepTimer) clearTimeout(sleepTimer);
    if (!animated || document.hidden) return;
    sleepTimer = setTimeout(() => {
      if (!animated || document.hidden) return;
      renderPose("Sleep");
    }, IDLE_SLEEP_MS);
  }

  function scheduleReturnToRest(delayMs = RETURN_TO_REST_MS) {
    if (restTimer) clearTimeout(restTimer);
    if (!animated || document.hidden) return;
    restTimer = setTimeout(() => {
      if (!animated || document.hidden) return;
      renderPose("Peek up");
      scheduleIdleSleep();
    }, delayMs);
  }

  function reactToKeystroke() {
    if (!animated || document.hidden) return;

    const now = performance.now();
    // Wake immediately if sleeping.
    if (currentPoseName === "Sleep") {
      lastReactionTime = now;
      renderPose("Peek up");
      scheduleReturnToRest(900);
      scheduleIdleSleep();
      return;
    }

    // Coalesce rapid keystrokes so motion stays smooth at typing speed.
    if (now - lastReactionTime < REACTION_COALESCE_MS) {
      scheduleReturnToRest();
      scheduleIdleSleep();
      return;
    }
    lastReactionTime = now;

    if (isReducedMotion()) {
      // Brief expression cue without positional travel
      renderPose("Peek up", "thocci-excited.svg");
      scheduleReturnToRest(650);
      scheduleIdleSleep();
      return;
    }

    const nextPose = REACTION_SEQUENCE[reactionIndex % REACTION_SEQUENCE.length];
    reactionIndex += 1;
    renderPose(nextPose);
    scheduleReturnToRest();
    scheduleIdleSleep();
  }

  function setAnimated(nextAnimated) {
    animated = Boolean(nextAnimated);
    updateMotionAttrs();
    if (!animated) {
      clearTimers();
      renderPose("Peek up");
    } else {
      scheduleIdleSleep();
    }
  }

  function onVisibilityChange() {
    if (document.hidden) {
      clearTimers();
    } else if (animated) {
      if (currentPoseName !== "Sleep") {
        scheduleIdleSleep();
      }
    }
  }

  function onReducedMotionChange() {
    updateMotionAttrs();
    renderPose(currentPoseName);
  }

  document.addEventListener("visibilitychange", onVisibilityChange);
  reducedMotionQuery.addEventListener("change", onReducedMotionChange);

  updateMotionAttrs();
  renderPose("Peek up");
  scheduleIdleSleep();

  return {
    reactToKeystroke,
    setAnimated,
    setPose(name) {
      renderPose(name);
    },
    destroy() {
      clearTimers();
      document.removeEventListener("visibilitychange", onVisibilityChange);
      reducedMotionQuery.removeEventListener("change", onReducedMotionChange);
    },
  };
}
