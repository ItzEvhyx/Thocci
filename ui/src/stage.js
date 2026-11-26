/**
 * Thocci persistent SVG mascot stage (850 x 304 logical coordinate space).
 *
 * Reuses bundled mascot artwork (`thocci-quick-actions.svg`, `thocci-sleeping.svg`,
 * `thocci-excited.svg`, and `thocci-pillow.svg`) in a single persistent SVG rig
 * driven by one `requestAnimationFrame` loop with elapsed time.
 *
 * Behavior:
 * - Sleeps on the pillow by default with subtle breathing, closed eyes, and 3 drifting Zs
 * - Wakes on the first physical or on-screen key press (stage clicks leave Ink sleeping)
 * - Builds cadence energy with ~670 ms decay; shuffles playful actions without repeats
 *   at bag boundaries (seeded first round with corner peeks + depth zoom)
 * - Peeks from all four corners (2 corners per peek sequence, cycling all 4 with
 *   distinct expressions) and performs visible 0.5x -> 1.6x -> 1.0x depth zooms
 * - Detects very fast typing (~9 presses in 960 ms), brakes with shocked round eyes,
 *   and interrupts ordinary chatter with a once-per-burst speed warning
 * - Smoothly slows and returns home to the pillow retaining momentum
 * - Honors Animate Thocci (off = static sleep, zero rAF/bubbles/reactions),
 *   prefers-reduced-motion (stays on pillow with expression & speech feedback only),
 *   document visibility, and Typing Challenge suspension
 */

import { loadTextAsset } from "./icons.js";

const SVG_NS = "http://www.w3.org/2000/svg";
const STAGE_BASE_WIDTH = 850;
const STAGE_HEIGHT = 304;
const HOME_X = 0.5;
const HOME_Y = 245;
const ENERGY_DECAY_SEC = 0.67;
const FAST_BURST_KEYS = 9;
const FAST_BURST_WINDOW_SEC = 0.96;
const SPEECH_COOLDOWN_SEC = 3.4;

const FALLBACK_BODY_PATH =
  "M14 106C5 100 0 90 0 79c0-12 5-24 13-35l13-27c3-7 11-8 15-2l9 12c12-6 26-10 43-10l19-14c6-5 13-3 16 3l24 43c5 10 8 20 8 31 0 13-6 24-17 29-17 7-39 10-63 10-26 0-51-3-66-13Z";
const FALLBACK_CLOSED_EYES_PATH = "M78 56c3 5 7 5 10 0m10 0c3 5 7 5 10 0";
const FALLBACK_HAPPY_EYES_PATH = "m76 63 6-6 6 6m9 0 6-6 6 6";

export const STAGE_PHRASES = Object.freeze({
  waking: ["Oh, hi!", "I'm up!", "Was that a key?"],
  alert: ["What are we typing?", "I'm listening."],
  peek: ["Psst. Over here.", "You found me!", "Boo."],
  pounce: ["Got it!", "Boing!"],
  curious: ["What was that?", "Hmm… interesting."],
  wiggle: ["Look what I can do!", "One more hop!"],
  zip: ["Zoomies!", "Catch me if you can."],
  braking: [
    "Hey slow down a bit!",
    "Whew! How many fingers do you have?",
    "My paws can’t keep up!",
  ],
  returning: ["Where's my pillow?", "That was fun."],
});

export const STAGE_GAMES = Object.freeze(["peek", "pounce", "curious", "wiggle", "zip"]);

export const STAGE_CORNERS = Object.freeze([
  { name: "top-left", side: -1, upper: true, expression: "curious" },
  { name: "bottom-right", side: 1, upper: false, expression: "excited" },
  { name: "top-right", side: 1, upper: true, expression: "shocked" },
  { name: "bottom-left", side: -1, upper: false, expression: "wink" },
]);

const ACTIVE_STATES = new Set([...STAGE_GAMES, "scamper", "braking"]);

const clamp = (v, lo = 0, hi = 1) => Math.min(hi, Math.max(lo, v));
const mix = (a, b, u) => a + (b - a) * u;
const ease = (u) => u ** 2 * (3 - 2 * u);
const pulse = (u) => Math.sin(Math.PI * clamp(u));
const follow = (current, target, rate, dt) =>
  dt <= 0 ? current : mix(current, target, 1 - Math.exp(-rate * dt));

function svgEl(tag, attrs = {}) {
  const el = document.createElementNS(SVG_NS, tag);
  for (const [k, v] of Object.entries(attrs)) {
    el.setAttribute(k, String(v));
  }
  return el;
}

/**
 * Pure, DOM-free mascot state machine & animation rig calculator.
 * Exported so Node unit tests can verify cadence, bag shuffling, corner peeks,
 * depth zoom, fast-burst warnings, reduced motion, and suspension cleanly.
 */
export function createMascotController({
  initialAnimated = true,
  initialReducedMotion = false,
  inputDebounceMs = 15,
  random = Math.random,
} = {}) {
  const phraseIndex = new Map();
  const recentKeys = [];

  let animated = Boolean(initialAnimated);
  let reduced = Boolean(initialReducedMotion);
  let suspended = false;

  let width = STAGE_BASE_WIDTH;
  let sceneScale = 1;

  let state = "sleeping";
  let time = 0;
  let entered = 0;
  let lastKey = -Infinity;
  let lastInputMs = -Infinity;
  let energy = 0;

  let x = HOME_X;
  let y = HOME_Y;
  let vx = 0;
  let vy = 0;
  let gaze = 0;
  let glanceY = 0;
  let lean = 0;
  let eye = 0;
  let grin = 0;
  let squeeze = 0.94;
  let lift = 0;
  let snooze = 1;
  let gait = 0;
  let surprised = 0;
  let squint = 0;

  let lastSpeech = -Infinity;
  let speechUntil = 0;
  let speechText = "";
  let speechSeq = 0;
  let warned = false;

  // Seed first round so corner peeks and depth zoom are discovered right away.
  let gameBag = ["zip", "peek"];
  let lastGame = "";
  let cornerIndex = 0;
  let peeksLeft = 0;
  let corner = STAGE_CORNERS[0];
  let zoomLeg = 0;
  let zoom = 1;
  let move = null;

  function clearSpeech() {
    speechUntil = 0;
    speechText = "";
  }

  function resetToStaticSleep() {
    move = null;
    state = "sleeping";
    entered = time;
    energy = 0;
    lastKey = -Infinity;
    recentKeys.length = 0;
    warned = false;
    x = HOME_X;
    y = HOME_Y;
    vx = 0;
    vy = 0;
    gaze = 0;
    glanceY = 0;
    lean = 0;
    eye = 0;
    grin = 0;
    squeeze = 0.94;
    lift = 0;
    snooze = 1;
    surprised = 0;
    squint = 0;
    zoom = 1;
    clearSpeech();
  }

  function speak(cue, urgent = false) {
    if (!animated || suspended) return false;
    const options = STAGE_PHRASES[cue];
    if (!options) return false;
    if (!urgent && time - lastSpeech < SPEECH_COOLDOWN_SEC) return false;
    // Save ordinary chatter for moments both ears are roughly within the stage.
    if (!urgent && cue !== "peek" && (x < 0.12 || x > 0.88)) return false;

    const index = phraseIndex.get(cue) ?? 0;
    phraseIndex.set(cue, (index + 1) % options.length);
    speechText = options[index];
    speechSeq += 1;
    lastSpeech = time;
    speechUntil = time + (urgent ? 2.85 : 2.2);
    return true;
  }

  function enter(next) {
    const changed = state !== next;
    state = next;
    entered = time;
    if (!changed) return;
    if (next === "sleeping") {
      speechUntil = time;
    } else if (next !== "peek" && next !== "braking") {
      speak(next, false);
    }
  }

  function travel(next, nextX, nextY, length, arc = 0) {
    enter(next);
    move = { x, y, vx, vy, nextX, nextY, length, arc, start: time };
  }

  function updateTravel(dt) {
    if (!move) return false;
    const u = clamp((time - move.start) / move.length);
    const carry = u * (1 - u) ** 2;
    const bend = 16 * u ** 2 * (1 - u) ** 2;
    const nextX =
      mix(move.x, move.nextX, ease(u)) + carry * clamp(move.vx * move.length, -0.25, 0.25);
    const nextY =
      mix(move.y, move.nextY, ease(u)) +
      carry * clamp(move.vy * move.length, -78, 78) +
      bend * move.arc;
    if (dt > 0) {
      vx = (nextX - x) / dt;
      vy = (nextY - y) / dt;
    }
    x = nextX;
    y = nextY;
    if (u < 1) return false;
    move = null;
    vx = 0;
    vy = 0;
    return true;
  }

  function approachCorner() {
    corner = STAGE_CORNERS[cornerIndex];
    const exitX = corner.side < 0 ? -0.04 : 1.04;
    const exitY = corner.upper ? -11 : 415;
    const distance = Math.hypot((exitX - x) * width, exitY - y);
    travel(
      "scamper",
      exitX,
      exitY,
      clamp(0.31 + distance / 1100, 0.55, 1.04),
      corner.upper ? -24 : 17,
    );
  }

  function play() {
    if (!gameBag.length) {
      gameBag = [...STAGE_GAMES];
      for (let i = gameBag.length - 1; i > 0; i--) {
        const j = Math.floor(random() * (i + 1));
        [gameBag[i], gameBag[j]] = [gameBag[j], gameBag[i]];
      }
      if (gameBag[gameBag.length - 1] === lastGame) {
        const tail = gameBag.length - 1;
        [gameBag[0], gameBag[tail]] = [gameBag[tail], gameBag[0]];
      }
    }
    lastGame = gameBag.pop();
    const towardCenter = x < 0.5 ? 1 : -1;

    if (lastGame === "peek") {
      peeksLeft = 2;
      approachCorner();
    } else if (lastGame === "pounce") {
      travel("pounce", clamp(x + 0.31 * towardCenter, 0.18, 0.82), 255, 0.88, 5);
    } else if (lastGame === "curious") {
      travel("curious", clamp(x + 0.19 * towardCenter, 0.26, 0.74), 242, 0.67);
    } else if (lastGame === "wiggle") {
      travel("wiggle", clamp(x + 0.19 * towardCenter, 0.19, 0.81), 261, 0.95, 4);
    } else {
      zoomLeg = 0;
      travel("zip", towardCenter > 0 ? 0.78 : 0.22, 169, 0.66, -19);
    }
  }

  function advanceZoom() {
    zoomLeg += 1;
    if (zoomLeg === 1) {
      travel("zip", 0.5 + (x > 0.5 ? -0.06 : 0.06), 281, 0.78, 16);
    } else if (zoomLeg === 2) {
      travel("zip", x < 0.5 ? 0.62 : 0.38, 248, 0.5, -7);
    } else {
      play();
    }
  }

  function returnHome() {
    travel("returning", HOME_X, HOME_Y, 0.46 + Math.abs(HOME_X - x) * 1.17, 10);
  }

  function update(dt) {
    if (!animated || suspended) return;
    const age = time - entered;
    energy *= Math.exp(-dt / ENERGY_DECAY_SEC);
    const sprint = !reduced && energy > 1.21 && time - lastKey < 0.48;
    const slowing = energy < 0.71 && time - lastKey > 0.5;
    const arrived = updateTravel(dt);

    if (
      !warned &&
      state !== "waking" &&
      time - lastKey < 0.18 &&
      recentKeys.length >= FAST_BURST_KEYS &&
      time - recentKeys[recentKeys.length - FAST_BURST_KEYS] < FAST_BURST_WINDOW_SEC
    ) {
      warned = true;
      speak("braking", true);
      if (!reduced) {
        travel("braking", clamp(x + vx * 0.16, 0.18, 0.82), clamp(y, 221, 259), 0.58);
        return;
      }
    }

    if (state === "waking" && age > 0.53) {
      if (sprint) play();
      else enter("alert");
    } else if (state === "alert" && sprint) {
      play();
    } else if (state === "alert" && time - lastKey > 1.45) {
      enter("settling");
    } else if (ACTIVE_STATES.has(state)) {
      if (slowing && state !== "braking") {
        returnHome();
      } else if (state === "scamper" && arrived) {
        const rim = corner.side < 0 ? 0 : 1;
        const inset = corner.side < 0 ? 31 : 50;
        travel(
          "peek",
          rim - (corner.side * inset) / width,
          corner.upper ? 109 : 334,
          0.43,
        );
      } else if (state === "peek" && arrived) {
        cornerIndex = (cornerIndex + 1) % STAGE_CORNERS.length;
        peeksLeft -= 1;
        speak("peek", false);
      } else if (state === "zip" && arrived && sprint) {
        advanceZoom();
      } else if (
        !move &&
        age > ({ peek: 1.43, curious: 1.29, braking: 1.95 }[state] ?? 0.65)
      ) {
        if (sprint && state === "peek" && peeksLeft > 0) approachCorner();
        else if (sprint) play();
        else returnHome();
      }
    } else if (state === "returning") {
      if (sprint) play();
      else if (arrived) enter(time - lastKey < 0.82 ? "alert" : "settling");
    } else if (state === "settling" && age > 0.8) {
      enter("sleeping");
    }
  }

  function computePose(dt) {
    const staticOff = !animated;
    const noMotion = staticOff || reduced;
    const age = time - entered;
    const atRest = state === "sleeping" || state === "settling";
    const peeking = !noMotion && state === "peek";
    const cornerFace = peeking ? corner.expression : "";
    const speed = Math.hypot(vx * width, vy);
    const pace = noMotion ? 0 : clamp(speed / 475);
    const speedSign = Math.sign(vx);

    const crouch = !noMotion && state === "waking" ? pulse(age / 0.43) : 0;
    const leap = !noMotion && state === "pounce" ? pulse((age - 0.1) / 0.51) : 0;
    const landing = !noMotion && state === "pounce" ? pulse((age - 0.54) / 0.18) : 0;
    const bounces =
      !noMotion && state === "wiggle"
        ? 0.74 * pulse((age - 0.08) / 0.27) + 0.6 * pulse((age - 0.43) / 0.31)
        : 0;
    const inspect =
      !noMotion && state === "curious"
        ? Math.sin((age - 0.1) * 5.5) * pulse(age / 1.31)
        : 0;

    const gasp =
      !staticOff &&
      !atRest &&
      (state === "braking" ||
        cornerFace === "shocked" ||
        (reduced && warned && time - lastSpeech < 1.25));

    const restingBreath = noMotion ? 0 : Math.sin(time * 1.66) * 0.015;
    const twitch = noMotion ? 0 : pulse((time - lastKey) / 0.18) * 0.045;
    const blinkPhase = (time + 0.25) % 3.9;
    const blink =
      !noMotion && !atRest && blinkPhase < 0.13 ? pulse(blinkPhase / 0.13) : 0;

    const delighted =
      cornerFace === "excited"
        ? ease(clamp((age - 0.33) / 0.24)) * (1 - ease(clamp((age - 1.07) / 0.29)))
        : 0;
    const zoomJoy =
      !noMotion && state === "zip" && zoomLeg === 1
        ? ease(clamp((age - 0.08) / 0.24)) * (1 - ease(clamp((age - 0.56) / 0.25)))
        : 0;
    const reducedHappy =
      !staticOff &&
      reduced &&
      !gasp &&
      state === "alert" &&
      energy > 1.35 &&
      time - lastKey < 0.42
        ? 1
        : 0;

    const smile = staticOff
      ? 0
      : reduced
        ? reducedHappy
        : Math.max(
            state === "pounce" ? pulse((age - 0.13) / 0.57) : 0,
            bounces,
            delighted,
            zoomJoy,
          );

    const stare = peeking ? -corner.side * 9 : inspect * 9 + speedSign * pace * 9;
    const peekTilt = -corner.side * (10 + Math.sin(age * 8) * 3);
    const tilt = peeking
      ? peekTilt
      : speedSign * 13 * pace + inspect * 14 + bounces * 7 * Math.sin(age * 22);

    gaze = noMotion ? 0 : follow(gaze, stare, 11, dt);
    glanceY = noMotion ? 0 : follow(glanceY, peeking ? (corner.upper ? 5 : -7) : 0, 10, dt);
    lean = noMotion ? 0 : follow(lean, tilt, 11, dt);

    const eyeTarget = staticOff
      ? 0
      : state === "sleeping"
        ? 0
        : atRest
          ? reduced
            ? 0
            : 0.55 * (1 - ease(clamp(age / 0.5)))
          : 1;
    eye = staticOff ? 0 : reduced ? eyeTarget : follow(eye, eyeTarget, 19, dt);
    grin = staticOff ? 0 : reduced ? smile : follow(grin, smile, 17, dt);
    surprised = staticOff ? 0 : reduced ? (gasp ? 1 : 0) : follow(surprised, gasp ? 1 : 0, 13, dt);

    const wink = cornerFace === "wink" ? 0.9 * pulse((age - 0.36) / 0.57) : 0;
    squint = noMotion
      ? 0
      : follow(
          squint,
          peeking ? -corner.side * (cornerFace === "curious" ? 0.47 : wink) : inspect * 0.48,
          14,
          dt,
        );

    const squeezeTarget = noMotion
      ? 0.94
      : atRest
        ? 0.94
        : 1 -
          crouch * 0.12 +
          leap * 0.085 -
          landing * 0.13 -
          Math.abs(inspect) * 0.08 +
          bounces * 0.065;
    squeeze = noMotion ? 0.94 : follow(squeeze, squeezeTarget, 12, dt);

    if (!noMotion) {
      gait += dt * pace * 28;
    }
    const hop = Math.abs(Math.sin(gait)) * pace * (peeking ? 2 : 15);
    lift = noMotion ? 0 : follow(lift, hop + leap * 41 + bounces * 35, 24, dt);
    snooze = staticOff
      ? 1
      : reduced
        ? state === "sleeping"
          ? 1
          : 0
        : follow(snooze, state === "sleeping" ? 1 : 0, 6, dt);

    const stageDepth = clamp(0.94 + (y - HOME_Y) * 0.0013, 0.69, 1.09);
    const zoomTarget =
      !noMotion && state === "zip"
        ? [0.57, 1.59, 1][zoomLeg]
        : peeking
          ? corner.upper
            ? 1.36
            : 1.12
          : 1;
    zoom = noMotion ? 1 : follow(zoom, zoomTarget, 6.8, dt);

    const depth = noMotion ? 0.94 : stageDepth * zoom;
    const scaleX = depth * (1 + (1 - squeeze) * 0.38 + pace * 0.036);
    const scaleY = depth * (squeeze + restingBreath * (atRest ? 1 : 0.35) + twitch);
    const cx = x * width;

    const openness = eye * (1 - blink * 0.99);
    const eyeHeight = 0.18 + openness * 0.82;

    const zzz = [0, 1, 2].map((index) => {
      const phase = noMotion ? (index + 1) / 4 : (time / 3.05 + index / 3 + 0.12) % 1;
      const baseOpacity = noMotion
        ? 0.72
        : ease(clamp(phase * 6)) * (1 - ease(clamp((phase - 0.64) / 0.36)));
      const scale = 0.75 + phase * 0.65;
      return {
        tx: width / 2 + 43 + 47 * phase,
        ty: 143 - 56 * phase,
        scale,
        opacity: baseOpacity * snooze,
      };
    });

    const speaking = animated && !suspended && time <= speechUntil && Boolean(speechText);
    const faceY = y - lift - 59 * scaleY;
    const offstage = faceY < -11 || faceY > 313 || cx < -24 || cx > width + 24;

    return {
      state,
      cornerName: state === "peek" ? corner.name : null,
      x,
      y,
      cx,
      lift,
      lean,
      depth,
      zoom,
      scaleX,
      scaleY,
      gaze,
      glanceY,
      eyeHeight,
      squint,
      openOpacity: openness * (1 - grin) * (1 - surprised),
      closedOpacity: (1 - openness) * (1 - grin) * (1 - surprised),
      happyOpacity: grin,
      shockedOpacity: surprised * (1 - grin),
      shadowCx: cx,
      shadowCy: y + 1,
      shadowRx: Math.max(8, (63 - lift * 0.42) * depth),
      shadowOpacity: Math.max(0.05, 0.23 - lift * 0.003),
      zzz,
      snooze,
      speaking,
      speechText,
      speechSeq,
      offstage,
      width,
      sceneScale,
    };
  }

  function step(dt) {
    const safeDt = clamp(dt, 0, 0.08);
    if (animated && !suspended && safeDt > 0) {
      time += safeDt;
      update(safeDt);
    }
    return computePose(safeDt);
  }

  function triggerKeystroke(nowMs = performance.now()) {
    if (!animated || suspended) return false;
    if (inputDebounceMs > 0 && nowMs - lastInputMs < inputDebounceMs) return false;
    lastInputMs = nowMs;

    if (time - lastKey > 1.2) warned = false;
    lastKey = time;
    recentKeys.push(time);
    if (recentKeys.length > FAST_BURST_KEYS) recentKeys.shift();

    energy = clamp(energy + 0.76, 0, 3.2);
    if (state === "sleeping" || state === "settling") {
      enter("waking");
    }
    return true;
  }

  function setAnimated(nextAnimated) {
    const next = Boolean(nextAnimated);
    if (next === animated) return false;
    animated = next;
    if (!animated) {
      resetToStaticSleep();
    } else {
      energy = 0;
      lastKey = -Infinity;
      lastInputMs = -Infinity;
      recentKeys.length = 0;
      warned = false;
    }
    return true;
  }

  function setReducedMotion(nextReduced) {
    const next = Boolean(nextReduced);
    if (next === reduced) return false;
    reduced = next;
    if (reduced) {
      move = null;
      x = HOME_X;
      y = HOME_Y;
      vx = 0;
      vy = 0;
      lift = 0;
      lean = 0;
      gaze = 0;
      glanceY = 0;
      squeeze = 0.94;
      grin = 0;
      zoom = 1;
      if (animated && state !== "sleeping") {
        enter("alert");
      }
    }
    return true;
  }

  function setSuspended(nextSuspended) {
    const next = Boolean(nextSuspended);
    if (next === suspended) return false;
    suspended = next;
    if (suspended) {
      resetToStaticSleep();
    } else {
      energy = 0;
      lastKey = -Infinity;
      lastInputMs = -Infinity;
      recentKeys.length = 0;
      warned = false;
    }
    return true;
  }

  function setStageSize(nextWidth, nextSceneScale) {
    if (Number.isFinite(nextWidth) && nextWidth > 0) {
      width = nextWidth;
    }
    if (Number.isFinite(nextSceneScale) && nextSceneScale > 0) {
      sceneScale = nextSceneScale;
    }
  }

  function clearTransientCadence() {
    energy = 0;
    lastKey = -Infinity;
    lastInputMs = -Infinity;
    recentKeys.length = 0;
  }

  function getState() {
    return {
      animated,
      reduced,
      suspended,
      state,
      corner: state === "peek" ? corner.name : null,
      x,
      y,
      energy,
      zoom,
      zoomLeg,
      warned,
      speaking: animated && !suspended && time <= speechUntil && Boolean(speechText),
      speechText: time <= speechUntil ? speechText : "",
      lastGame,
      time,
    };
  }

  return {
    step,
    computePose,
    triggerKeystroke,
    setAnimated,
    setReducedMotion,
    setSuspended,
    setStageSize,
    clearTransientCadence,
    getState,
  };
}

function extractMascotArt(rawOpen, rawSleep, rawExcited, rawPillow) {
  const parser = new DOMParser();

  const openDoc = parser.parseFromString(rawOpen, "image/svg+xml");
  const sleepDoc = parser.parseFromString(rawSleep, "image/svg+xml");
  const excitedDoc = parser.parseFromString(rawExcited, "image/svg+xml");
  const pillowDoc = parser.parseFromString(rawPillow, "image/svg+xml");

  const bodyPathD =
    openDoc.querySelector("#thocci-body path")?.getAttribute("d") || FALLBACK_BODY_PATH;
  const openEyeRects = Array.from(openDoc.querySelectorAll("#thocci-eyes rect")).map((r) => ({
    x: r.getAttribute("x") || "76",
    y: r.getAttribute("y") || "49",
    width: r.getAttribute("width") || "11",
    height: r.getAttribute("height") || "20",
    rx: r.getAttribute("rx") || "5.5",
  }));
  const closedEyePathD =
    sleepDoc.querySelector("#thocci-eyes path")?.getAttribute("d") || FALLBACK_CLOSED_EYES_PATH;
  const happyEyePathD =
    excitedDoc.querySelector("#thocci-eyes path")?.getAttribute("d") || FALLBACK_HAPPY_EYES_PATH;

  const pillowDefs = Array.from(pillowDoc.querySelectorAll("defs > *"));
  const pillowShapes = Array.from(pillowDoc.documentElement.children).filter(
    (el) => el.tagName.toLowerCase() !== "defs",
  );

  return {
    bodyPathD,
    openEyeRects:
      openEyeRects.length === 2
        ? openEyeRects
        : [
            { x: "76", y: "49", width: "11", height: "20", rx: "5.5" },
            { x: "97", y: "48", width: "11", height: "20", rx: "5.5" },
          ],
    closedEyePathD,
    happyEyePathD,
    pillowDefs,
    pillowShapes,
  };
}

export async function mountMascotStage(
  container,
  { initialTheme = "light", initialAnimated = true } = {},
) {
  const [openSvg, sleepSvg, excitedSvg, pillowSvg] = await Promise.all([
    loadTextAsset("./vendor/mascot/thocci-quick-actions.svg"),
    loadTextAsset("./vendor/mascot/thocci-sleeping.svg"),
    loadTextAsset("./vendor/mascot/thocci-excited.svg"),
    loadTextAsset("./vendor/mascot/thocci-pillow.svg"),
  ]);

  const art = extractMascotArt(openSvg, sleepSvg, excitedSvg, pillowSvg);

  // Build persistent SVG rig once: static pillow layer stays cached while Ink
  // animates on its own layer and idle sleep breathing/Zs run on GPU CSS layers.
  const pillowSvgEl = svgEl("svg", {
    class: "stage-svg stage-svg--pillow",
    viewBox: `0 0 ${STAGE_BASE_WIDTH} ${STAGE_HEIGHT}`,
    "aria-hidden": "true",
  });

  const defs = svgEl("defs");
  for (const node of art.pillowDefs) {
    defs.appendChild(document.importNode(node, true));
  }
  pillowSvgEl.appendChild(defs);

  const pillow = svgEl("g", {
    "data-pillow": "",
    transform: `translate(${STAGE_BASE_WIDTH / 2} 244)`,
  });
  for (const node of art.pillowShapes) {
    pillow.appendChild(document.importNode(node, true));
  }
  pillowSvgEl.appendChild(pillow);

  const svg = svgEl("svg", {
    class: "stage-svg stage-svg--cat",
    viewBox: `0 0 ${STAGE_BASE_WIDTH} ${STAGE_HEIGHT}`,
    role: "img",
    "aria-label": "Thocci, the black blob cat, naps on a pillow and reacts to your typing.",
  });

  const shadow = svgEl("ellipse", {
    "data-shadow": "",
    cx: String(STAGE_BASE_WIDTH / 2),
    cy: "243",
    rx: "62",
    ry: "8",
    fill: "#111114",
    opacity: "0.2",
  });
  svg.appendChild(shadow);

  const cat = svgEl("g", {
    "data-cat": "",
    transform: `translate(${STAGE_BASE_WIDTH / 2} ${HOME_Y}) scale(1 0.94)`,
  });
  const bodyOrigin = svgEl("g", { transform: "translate(-80 -119)" });

  bodyOrigin.appendChild(
    svgEl("path", {
      "data-body": "",
      fill: "#17181B",
      stroke: "var(--stage-cat-rim)",
      "stroke-width": "0.95",
      d: art.bodyPathD,
    }),
  );

  const face = svgEl("g", { "data-face": "" });

  const openEyes = svgEl("g", { "data-open": "", fill: "#FDFDFF", opacity: "0" });
  const leftEye = svgEl("rect", art.openEyeRects[0]);
  const rightEye = svgEl("rect", art.openEyeRects[1]);
  openEyes.append(leftEye, rightEye);

  const shockedEyes = svgEl("g", { "data-shocked": "", fill: "#FDFDFF", opacity: "0" });
  shockedEyes.append(
    svgEl("circle", { cx: "80", cy: "57", r: "8" }),
    svgEl("circle", { cx: "104", cy: "56", r: "8" }),
  );

  const closedEyes = svgEl("path", {
    "data-closed": "",
    d: art.closedEyePathD,
    fill: "none",
    stroke: "#fff",
    "stroke-width": "2.7",
    "stroke-linecap": "round",
    opacity: "1",
  });

  const happyEyes = svgEl("path", {
    "data-happy": "",
    d: art.happyEyePathD,
    fill: "none",
    stroke: "#fff",
    "stroke-width": "2.8",
    "stroke-linecap": "round",
    "stroke-linejoin": "round",
    opacity: "0",
  });

  face.append(openEyes, shockedEyes, closedEyes, happyEyes);
  bodyOrigin.appendChild(face);
  cat.appendChild(bodyOrigin);
  svg.appendChild(cat);

  // GPU-composited HTML Z glyphs avoid per-frame SVG font rasterization in WebKitGTK.
  const zzzLayer = document.createElement("div");
  zzzLayer.className = "stage-zzz-layer";
  zzzLayer.setAttribute("data-zzz", "");
  zzzLayer.setAttribute("aria-hidden", "true");
  const marks = [0, 1, 2].map(() => {
    const z = document.createElement("span");
    z.className = "stage-z";
    z.textContent = "Z";
    zzzLayer.appendChild(z);
    return z;
  });

  const bubble = document.createElement("div");
  bubble.className = "stage-bubble";
  bubble.setAttribute("role", "status");
  bubble.setAttribute("aria-live", "polite");
  bubble.setAttribute("aria-atomic", "true");

  container.replaceChildren(pillowSvgEl, svg, zzzLayer, bubble);

  const reducedMotionQuery = window.matchMedia("(prefers-reduced-motion: reduce)");
  const controller = createMascotController({
    initialAnimated,
    initialReducedMotion: reducedMotionQuery.matches,
  });

  const attrCache = new WeakMap();
  function setAttr(el, name, value) {
    let map = attrCache.get(el);
    if (!map) {
      map = Object.create(null);
      attrCache.set(el, map);
    }
    if (map[name] === value) return;
    map[name] = value;
    el.setAttribute(name, value);
  }

  let currentTheme = initialTheme;
  let rafId = 0;
  let sleepTimerId = 0;
  let previousFrame = 0;
  let destroyed = false;
  let renderedSpeechSeq = 0;
  let bubbleHalf = 0;
  let bubbleHeight = 0;

  function syncAttributes(pose) {
    const snap = controller.getState();
    setAttr(container, "data-animated", snap.animated ? "true" : "false");
    setAttr(container, "data-reduced-motion", snap.reduced ? "true" : "false");
    setAttr(container, "data-theme", currentTheme);
    if (container.dataset.state !== pose.state) {
      container.dataset.state = pose.state;
    }
    if (pose.cornerName) {
      if (container.dataset.corner !== pose.cornerName) {
        container.dataset.corner = pose.cornerName;
      }
    } else if ("corner" in container.dataset) {
      delete container.dataset.corner;
    }
  }

  function applyPose(pose) {
    syncAttributes(pose);

    setAttr(
      cat,
      "transform",
      `translate(${pose.cx.toFixed(2)} ${(pose.y - pose.lift).toFixed(2)}) rotate(${pose.lean.toFixed(2)}) scale(${pose.scaleX.toFixed(4)} ${pose.scaleY.toFixed(4)})`,
    );
    setAttr(shadow, "cx", pose.shadowCx.toFixed(2));
    setAttr(shadow, "cy", pose.shadowCy.toFixed(2));
    setAttr(shadow, "rx", pose.shadowRx.toFixed(2));
    setAttr(shadow, "opacity", pose.shadowOpacity.toFixed(3));

    setAttr(
      face,
      "transform",
      `translate(${pose.gaze.toFixed(2)} ${pose.glanceY.toFixed(2)})`,
    );
    setAttr(
      openEyes,
      "transform",
      `translate(0 59) scale(1 ${pose.eyeHeight.toFixed(4)}) translate(0 -59)`,
    );
    setAttr(
      leftEye,
      "transform",
      `translate(0 59) scale(1 ${(1 - Math.max(0, pose.squint)).toFixed(4)}) translate(0 -59)`,
    );
    setAttr(
      rightEye,
      "transform",
      `translate(0 58) scale(1 ${(1 + Math.min(0, pose.squint)).toFixed(4)}) translate(0 -58)`,
    );

    setAttr(openEyes, "opacity", pose.openOpacity.toFixed(3));
    setAttr(closedEyes, "opacity", pose.closedOpacity.toFixed(3));
    setAttr(happyEyes, "opacity", pose.happyOpacity.toFixed(3));
    setAttr(shockedEyes, "opacity", pose.shockedOpacity.toFixed(3));

    const zScale = pose.sceneScale;
    const zOriginX = pose.width / 2 + 43;
    const zOriginY = 143;
    for (let i = 0; i < marks.length; i++) {
      const m = pose.zzz[i];
      const dx = (m.tx - zOriginX) * zScale;
      const dy = (m.ty - zOriginY) * zScale;
      const op = m.opacity < 0.004 ? "0" : m.opacity.toFixed(3);
      const tf = `translate3d(${dx.toFixed(1)}px, ${dy.toFixed(1)}px, 0) scale(${m.scale.toFixed(3)})`;
      if (marks[i].style.opacity !== op) marks[i].style.opacity = op;
      if (op !== "0" && marks[i].style.transform !== tf) {
        marks[i].style.transform = tf;
      }
    }

    // Update speech bubble text at most once per cue so the live region stays clean.
    if (pose.speaking) {
      if (pose.speechSeq !== renderedSpeechSeq) {
        renderedSpeechSeq = pose.speechSeq;
        bubble.textContent = pose.speechText;
        bubbleHalf = bubble.offsetWidth / 2;
        bubbleHeight = bubble.offsetHeight;
      }
      bubble.setAttribute("data-speaking", "");

      if (bubbleHalf > 0) {
        const stageW = pose.width * pose.sceneScale;
        const stageH = STAGE_HEIGHT * pose.sceneScale;
        const target = pose.cx * pose.sceneScale;
        const above = (pose.y - pose.lift - 126 * pose.scaleY) * pose.sceneScale;
        const flyClearBottom = Math.max(32, stageH * 0.19);
        const beside =
          (above < bubbleHeight + 7 && (pose.x < 0.26 || pose.x > 0.74)) ||
          (above < bubbleHeight + flyClearBottom && (pose.x < 0.24 || pose.x > 0.76));
        const inward = pose.x < 0.5 ? 1 : -1;
        const bubbleTarget = beside
          ? target + inward * (pose.depth * 66 * pose.sceneScale + bubbleHalf + 7)
          : target;
        const left = clamp(
          bubbleTarget,
          bubbleHalf + 8,
          Math.max(bubbleHalf + 8, stageW - bubbleHalf - 8),
        );
        const rawTop = beside
          ? Math.max(
              bubbleHeight + 8,
              (pose.y - pose.lift - 52 * pose.scaleY) * pose.sceneScale + bubbleHeight / 2,
            )
          : Math.max(bubbleHeight + 8, above);
        const top = clamp(rawTop, bubbleHeight + 8, Math.max(bubbleHeight + 8, stageH - 8));

        bubble.style.left = `${left.toFixed(1)}px`;
        bubble.style.top = `${top.toFixed(1)}px`;
        bubble.style.setProperty(
          "--tail",
          `${clamp(bubbleHalf + target - left, 12, Math.max(12, 2 * bubbleHalf - 12)).toFixed(1)}px`,
        );
        bubble.dataset.tail = beside ? (inward > 0 ? "left" : "right") : "bottom";
        bubble.toggleAttribute("data-offstage", pose.offstage);
      }
    } else {
      bubble.removeAttribute("data-speaking");
      bubble.removeAttribute("data-offstage");
      if (!pose.speechText && bubble.textContent) {
        bubble.textContent = "";
      }
    }
  }

  function isSettledSleep(pose, snap) {
    return (
      pose.state === "sleeping" &&
      !pose.speaking &&
      pose.snooze >= 0.995 &&
      Math.abs(pose.x - HOME_X) < 0.002 &&
      Math.abs(pose.y - HOME_Y) < 0.5 &&
      Math.abs(pose.lift) < 0.3 &&
      Math.abs(pose.lean) < 0.3 &&
      Math.abs(pose.zoom - 1) < 0.005 &&
      pose.closedOpacity >= 0.99 &&
      !snap.suspended
    );
  }

  function stopLoop() {
    if (rafId) {
      cancelAnimationFrame(rafId);
      rafId = 0;
    }
    if (sleepTimerId) {
      clearTimeout(sleepTimerId);
      sleepTimerId = 0;
    }
    svg.style.transform = "";
    previousFrame = 0;
  }

  function frame(stamp) {
    rafId = 0;
    const snap = controller.getState();
    if (destroyed || !snap.animated || snap.suspended || document.hidden) {
      svg.style.transform = "";
      previousFrame = 0;
      return;
    }
    const dt = clamp((stamp - (previousFrame || stamp - 16)) / 1000, 0.001, 0.075);
    previousFrame = stamp;
    const pose = controller.step(dt);

    // Under reduced motion, sleeping is completely static -> park loop until next keystroke.
    if (snap.reduced && pose.state === "sleeping" && !pose.speaking) {
      svg.style.transform = "";
      applyPose(pose);
      previousFrame = 0;
      return;
    }

    // Once fully settled asleep on the pillow, keep Ink's SVG bitmap completely
    // frozen (zero SVG re-rasterization!) and apply the tiny breathing scaleY +
    // Z drift via GPU-composited CSS transforms at a calm ~15 fps (66 ms).
    if (isSettledSleep(pose, snap)) {
      const breathRatio = pose.scaleY / (0.94 * 0.94);
      svg.style.transform = `translate3d(0, 0, 0) scaleY(${breathRatio.toFixed(4)})`;
      // Update only the 3 GPU-composited HTML Z spans (zero SVG attribute writes).
      const zScale = pose.sceneScale;
      const zOriginX = pose.width / 2 + 43;
      const zOriginY = 143;
      for (let i = 0; i < marks.length; i++) {
        const m = pose.zzz[i];
        const dx = (m.tx - zOriginX) * zScale;
        const dy = (m.ty - zOriginY) * zScale;
        marks[i].style.transform = `translate3d(${dx.toFixed(1)}px, ${dy.toFixed(1)}px, 0) scale(${m.scale.toFixed(3)})`;
        marks[i].style.opacity = m.opacity.toFixed(3);
      }
      sleepTimerId = setTimeout(() => {
        sleepTimerId = 0;
        if (!destroyed && !rafId) {
          rafId = requestAnimationFrame(frame);
        }
      }, 66);
      return;
    }

    if (svg.style.transform) svg.style.transform = "";
    applyPose(pose);
    rafId = requestAnimationFrame(frame);
  }

  function startLoop() {
    const snap = controller.getState();
    if (destroyed || rafId || !snap.animated || snap.suspended || document.hidden) return;
    if (sleepTimerId) {
      clearTimeout(sleepTimerId);
      sleepTimerId = 0;
    }
    previousFrame = 0;
    rafId = requestAnimationFrame(frame);
  }

  function measureStage() {
    const bounds = svg.getBoundingClientRect();
    if (!bounds.width || !bounds.height) return;
    const logicalWidth = (STAGE_HEIGHT * bounds.width) / bounds.height;
    const scale = bounds.height / STAGE_HEIGHT;
    controller.setStageSize(logicalWidth, scale);
    const vb = `0 0 ${logicalWidth.toFixed(2)} ${STAGE_HEIGHT}`;
    setAttr(pillowSvgEl, "viewBox", vb);
    setAttr(svg, "viewBox", vb);
    setAttr(pillow, "transform", `translate(${(logicalWidth / 2).toFixed(2)} 244)`);
    container.style.setProperty("--stage-scale", scale.toFixed(4));
    bubbleHalf = bubble.offsetWidth / 2;
    bubbleHeight = bubble.offsetHeight;
    applyPose(controller.computePose(0));
  }

  function reactToKeystroke() {
    if (destroyed || document.hidden) return;
    const accepted = controller.triggerKeystroke(performance.now());
    if (accepted) {
      if (svg.style.transform) svg.style.transform = "";
      if (!rafId || sleepTimerId) {
        startLoop();
      }
    }
  }

  function setAnimated(nextAnimated) {
    const changed = controller.setAnimated(nextAnimated);
    if (!changed) return;
    const snap = controller.getState();
    if (!snap.animated) {
      stopLoop();
      renderedSpeechSeq = 0;
      applyPose(controller.computePose(0));
    } else {
      applyPose(controller.computePose(0));
      startLoop();
    }
  }

  function setSuspended(nextSuspended) {
    const changed = controller.setSuspended(nextSuspended);
    if (!changed) return;
    const snap = controller.getState();
    if (snap.suspended) {
      stopLoop();
      renderedSpeechSeq = 0;
      applyPose(controller.computePose(0));
    } else {
      measureStage();
      applyPose(controller.computePose(0));
      startLoop();
    }
  }

  function setTheme(nextTheme) {
    if (!nextTheme || nextTheme === currentTheme) return;
    currentTheme = nextTheme;
    container.setAttribute("data-theme", currentTheme);
  }

  function onVisibilityChange() {
    if (document.hidden) {
      stopLoop();
      controller.clearTransientCadence();
      bubble.removeAttribute("data-speaking");
      bubble.removeAttribute("data-offstage");
    } else {
      controller.clearTransientCadence();
      measureStage();
      startLoop();
    }
  }

  function onReducedMotionChange() {
    const changed = controller.setReducedMotion(reducedMotionQuery.matches);
    if (changed) {
      applyPose(controller.computePose(0));
    }
  }

  const resizeObserver =
    typeof ResizeObserver === "function" ? new ResizeObserver(measureStage) : null;
  resizeObserver?.observe(svg);
  document.addEventListener("visibilitychange", onVisibilityChange);
  reducedMotionQuery.addEventListener("change", onReducedMotionChange);

  measureStage();
  applyPose(controller.computePose(0));
  startLoop();

  return {
    reactToKeystroke,
    setAnimated,
    setSuspended,
    setTheme,
    getState: () => controller.getState(),
    destroy() {
      if (destroyed) return;
      destroyed = true;
      stopLoop();
      resizeObserver?.disconnect();
      document.removeEventListener("visibilitychange", onVisibilityChange);
      reducedMotionQuery.removeEventListener("change", onReducedMotionChange);
    },
  };
}
