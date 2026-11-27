import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { KeyHoldTracker } from "../src/keyboard.js";
import {
  filterCatalog,
  isPointerBlurControl,
  isTextEditableElement,
} from "../src/dashboard.js";
import { computeQuickActionsGaze } from "../src/quick-actions.js";
import {
  TRINKET_CADENCE_RESET_MS,
  TRINKET_KEY_POSITIONS,
  TRINKET_PERCH_Y,
  createSidebarTrinketController,
} from "../src/sidebar-trinket.js";
import {
  STAGE_CORNERS,
  STAGE_GAMES,
  STAGE_PHRASES,
  createMascotController,
} from "../src/stage.js";
import {
  TRIVIA_PROMPTS,
  TYPING_DURATION_SECONDS,
  calculateTypingMetrics,
  pickRandomPrompt,
} from "../src/typing-game.js";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(__dirname, "..", "..");

test("pt-iso keyboard dataset has 79 unique keys, ISO Return, split arrows, and all glyphs", () => {
  const kbPath = path.join(root, "assets", "desktop", "keyboard", "pt-iso.json");
  const kb = JSON.parse(fs.readFileSync(kbPath, "utf8"));
  assert.equal(kb.keyCount, 79);
  assert.equal(kb.keys.length, 79);

  const codes = new Set(kb.keys.map((k) => k.code));
  assert.equal(codes.size, 79);
  assert.equal(kb.keys.find((k) => k.code === "Enter")?.shape, "isoReturn");
  assert.equal(kb.keys.find((k) => k.code === "ArrowUp")?.shape, "arrowUp");
  assert.equal(kb.keys.find((k) => k.code === "ArrowDown")?.shape, "arrowDown");
  assert.ok(codes.has("IntlBackslash"), "ISO key next to left shift must exist");
  assert.ok(
    kb.keys
      .find((k) => k.code === "Semicolon")
      ?.legends.some((l) => l.text === "Ç"),
    "Portuguese ISO Ç legend must exist",
  );

  for (const key of kb.keys) {
    for (const legend of key.legends || []) {
      if (legend.glyph) {
        const glyphFile = path.join(root, "assets", "desktop", "keyboard", legend.glyph);
        assert.ok(fs.existsSync(glyphFile), `Missing glyph file: ${legend.glyph}`);
      }
    }
  }
});

test("KeyHoldTracker tracks chords, pointer cancel, and blur resets cleanly", () => {
  const tracker = new KeyHoldTracker();

  // Physical chord: ShiftLeft + KeyA + Space
  assert.equal(tracker.physicalDown("ShiftLeft"), true);
  assert.equal(tracker.physicalDown("ShiftLeft"), false); // repeat ignored
  assert.equal(tracker.physicalDown("KeyA"), true);
  assert.equal(tracker.physicalDown("Space"), true);
  assert.equal(tracker.isPressed("ShiftLeft"), true);
  assert.equal(tracker.isPressed("KeyA"), true);
  assert.equal(tracker.isPressed("Space"), true);

  // Pointer audition on KeyF simultaneously with physical chord
  tracker.pointerDown(1, "KeyF");
  assert.equal(tracker.isPressed("KeyF"), true);
  assert.equal(tracker.pointerUp(1), "KeyF");
  assert.equal(tracker.isPressed("KeyF"), false);

  // Physical key held while pointer releases same key keeps pressed until physical up
  tracker.pointerDown(2, "KeyA");
  tracker.pointerUp(2);
  assert.equal(tracker.isPressed("KeyA"), true);
  tracker.physicalUp("KeyA");
  assert.equal(tracker.isPressed("KeyA"), false);

  // Window blur clears all held keys
  const cleared = tracker.clearAll();
  assert.ok(cleared.includes("ShiftLeft"));
  assert.ok(cleared.includes("Space"));
  assert.equal(tracker.isPressed("ShiftLeft"), false);
  assert.equal(tracker.isPressed("Space"), false);
});

test("filterCatalog filters by display_name, id, subtitle, and raw_name", () => {
  const sample = [
    {
      id: "creamy",
      display_name: "Creamy",
      subtitle: "Soft and muted",
      raw_name: "creamy (still getting better)",
      origin: "builtin",
    },
    {
      id: "cherry-mx",
      display_name: "Cherry MX Black",
      subtitle: "Linear · ABS keycaps",
      raw_name: "CherryMX Black - ABS keycaps",
      origin: "builtin",
    },
    {
      id: "imported-custom-thock",
      display_name: "Custom Thock",
      subtitle: "Imported WAV · 120 ms",
      raw_name: "Custom Thock",
      origin: "imported",
    },
  ];

  assert.equal(filterCatalog(sample, "").length, 3);
  assert.equal(filterCatalog(sample, "muted").length, 1);
  assert.equal(filterCatalog(sample, "muted")[0].id, "creamy");
  assert.equal(filterCatalog(sample, "abs").length, 1);
  assert.equal(filterCatalog(sample, "imported")[0].id, "imported-custom-thock");
  assert.equal(filterCatalog(sample, "nonexistent-pack").length, 0);
});

test("design tokens, stage poses, and bundled desktop assets match desktopui.md", () => {
  const tokens = JSON.parse(
    fs.readFileSync(path.join(root, "assets", "desktop", "tokens.json"), "utf8"),
  );
  assert.equal(tokens.motion.soundpacksOpen.durationMs, 320);
  assert.deepEqual(tokens.motion.soundpacksOpen.cubicBezier, [0.55, 0.03, 0.86, 0.5]);
  assert.equal(tokens.motion.soundpacksClose.durationMs, 260);
  assert.equal(tokens.motion.soundpacksClose.easing, "ease-out");
  assert.equal(tokens.motion.reducedMotion.soundpackTransitionMs, 0);
  assert.equal(tokens.quickActions.width, 288);
  assert.equal(tokens.quickActions.height, 282);
  assert.deepEqual(tokens.quickActions.switches.legends, [
    "S",
    "W",
    "I",
    "T",
    "C",
    "H",
    "E",
    "S",
  ]);

  const poses = JSON.parse(
    fs.readFileSync(path.join(root, "assets", "desktop", "mascot", "stage-poses.json"), "utf8"),
  );
  const poseNames = poses.poses.map((p) => p.name);
  assert.deepEqual(poseNames, ["Peek up", "Peek from side", "Slide", "Jump", "Sleep"]);
});

test("typing challenge has 20 trivia prompts (2 per category), 20s cap, and accurate WPM/accuracy math", () => {
  assert.equal(TYPING_DURATION_SECONDS, 20);
  assert.equal(TRIVIA_PROMPTS.length, 20);

  const byCategory = new Map();
  for (const p of TRIVIA_PROMPTS) {
    byCategory.set(p.category, (byCategory.get(p.category) || 0) + 1);
    const wordCount = p.text.trim().split(/\s+/).length;
    assert.ok(wordCount >= 65, `Prompt ${p.id} too short (${wordCount} words) for fast 20s typists`);
  }
  assert.equal(byCategory.size, 10);
  for (const [cat, count] of byCategory.entries()) {
    assert.equal(count, 2, `Expected 2 prompts for category ${cat}`);
  }

  const first = TRIVIA_PROMPTS[0];
  for (let i = 0; i < 25; i++) {
    const picked = pickRandomPrompt(first.id);
    assert.notEqual(picked.id, first.id);
  }

  const metrics = calculateTypingMetrics({
    correctChars: 150,
    totalKeystrokes: 160,
    correctKeystrokes: 144,
    elapsedMs: 20000,
  });
  assert.equal(metrics.wpm, 90);
  assert.equal(metrics.accuracy, 90);
});

test("mascot pillow asset exists and controller sleeps by default, wakes on key, and cycles all four corners + zoom", () => {
  const pillowSrc = path.join(root, "assets", "desktop", "mascot", "thocci-pillow.svg");
  assert.ok(fs.existsSync(pillowSrc), "thocci-pillow.svg must exist in assets/desktop/mascot/");

  // Deterministic RNG so bag shuffle is reproducible
  let seed = 0.25;
  const ctrl = createMascotController({
    inputDebounceMs: 0,
    random: () => {
      seed = (seed * 9301 + 49297) % 233280;
      return seed / 233280;
    },
  });

  // Default sleeping pose at center baseline y=245
  const initial = ctrl.computePose(0);
  assert.equal(initial.state, "sleeping");
  assert.equal(initial.x, 0.5);
  assert.equal(initial.y, 245);
  assert.equal(initial.closedOpacity, 1);

  // Single key wakes Ink promptly and speaks first waking phrase
  ctrl.triggerKeystroke(100);
  const awake = ctrl.step(0.05);
  assert.equal(awake.state, "waking");
  assert.equal(awake.speechText, STAGE_PHRASES.waking[0]);

  // Sustained steady typing (~5 cps, well below 9-in-960ms fast burst threshold)
  const visitedCorners = new Set();
  let sawRecede = false;
  let sawRush = false;
  let lastGameSeq = [];
  let prevGame = "";

  for (let i = 0; i < 950; i++) {
    if (i % 7 === 0) {
      ctrl.triggerKeystroke(200 + i * 32);
    }
    const pose = ctrl.step(0.032);
    const snap = ctrl.getState();
    if (pose.cornerName) visitedCorners.add(pose.cornerName);
    if (pose.state === "zip" && pose.zoom < 0.72) sawRecede = true;
    if (pose.state === "zip" && pose.zoom > 1.38) sawRush = true;
    if (snap.lastGame && snap.lastGame !== prevGame) {
      lastGameSeq.push(snap.lastGame);
      prevGame = snap.lastGame;
    }
  }

  // First seeded round must start with peek then zip
  assert.equal(lastGameSeq[0], "peek");
  assert.equal(lastGameSeq[1], "zip");
  // No back-to-back repeats across bag boundaries
  for (let i = 1; i < lastGameSeq.length; i++) {
    assert.notEqual(lastGameSeq[i], lastGameSeq[i - 1]);
  }
  assert.equal(visitedCorners.size, STAGE_CORNERS.length);
  assert.ok(sawRecede, "depth zoom should recede toward ~0.5x");
  assert.ok(sawRush, "depth zoom should rush toward ~1.6x");
  assert.equal(STAGE_GAMES.length, 5);

  // Stopping input slows Ink down and returns smoothly to sleeping on pillow
  for (let i = 0; i < 220; i++) {
    ctrl.step(0.04);
  }
  const settled = ctrl.getState();
  assert.equal(settled.state, "sleeping");
  assert.ok(Math.abs(settled.x - 0.5) < 0.01);
  assert.ok(Math.abs(settled.y - 245) < 1);
});

test("mascot controller detects fast bursts, interrupts chatter, respects Animate Thocci off, reduced motion, and suspension", () => {
  const ctrl = createMascotController({ inputDebounceMs: 0 });
  ctrl.triggerKeystroke(100);
  // Advance past waking so fast-burst detection is active
  for (let i = 0; i < 25; i++) ctrl.step(0.03);
  assert.equal(ctrl.getState().state, "alert");

  // 9 rapid presses within < 960 ms should trigger braking + first warning phrase
  for (let i = 0; i < 9; i++) {
    ctrl.triggerKeystroke(1000 + i * 50);
    ctrl.step(0.05);
  }
  const braking = ctrl.getState();
  assert.equal(braking.state, "braking");
  assert.equal(braking.speechText, "Hey slow down a bit!");

  // Pause > 1.2s and trigger a second fast burst -> rotates to second warning phrase
  for (let i = 0; i < 80; i++) ctrl.step(0.04);
  ctrl.triggerKeystroke(6000);
  for (let i = 0; i < 20; i++) ctrl.step(0.03);
  for (let i = 0; i < 9; i++) {
    ctrl.triggerKeystroke(7000 + i * 50);
    ctrl.step(0.05);
  }
  assert.equal(ctrl.getState().speechText, STAGE_PHRASES.braking[1]);

  // Animate Thocci off -> static sleeping pose, stops reactions & bubbles, idempotent
  assert.equal(ctrl.setAnimated(false), true);
  assert.equal(ctrl.setAnimated(false), false);
  const offSnap = ctrl.getState();
  assert.equal(offSnap.state, "sleeping");
  assert.equal(offSnap.speaking, false);
  assert.equal(ctrl.triggerKeystroke(9000), false);

  // Re-enable Animate Thocci + enable reduced motion -> stays on pillow with speech/expressions
  assert.equal(ctrl.setAnimated(true), true);
  assert.equal(ctrl.setReducedMotion(true), true);
  ctrl.triggerKeystroke(10000);
  const rmPose = ctrl.step(0.04);
  assert.equal(rmPose.state, "waking");
  assert.equal(rmPose.x, 0.5);
  assert.equal(rmPose.y, 245);
  assert.equal(rmPose.zoom, 1);
  assert.equal(rmPose.lift, 0);

  // Suspend (e.g. Typing Challenge) clears speech & ignores keystrokes, idempotent
  assert.equal(ctrl.setSuspended(true), true);
  assert.equal(ctrl.setSuspended(true), false);
  assert.equal(ctrl.getState().speaking, false);
  assert.equal(ctrl.triggerKeystroke(11000), false);
  assert.equal(ctrl.setSuspended(false), true);
});

test("pointer focus predicates release buttons/switches/sliders while preserving text inputs", () => {
  const mkEl = (tagName, attrs = {}) => ({
    tagName,
    type: attrs.type || "",
    isContentEditable: Boolean(attrs.contentEditable),
    getAttribute: (k) => attrs[k] ?? null,
  });

  assert.equal(isPointerBlurControl(mkEl("BUTTON")), true);
  assert.equal(isPointerBlurControl(mkEl("INPUT", { type: "range" })), true);
  assert.equal(isPointerBlurControl(mkEl("DIV", { role: "switch" })), true);
  assert.equal(isPointerBlurControl(mkEl("INPUT", { type: "search" })), false);
  assert.equal(isPointerBlurControl(mkEl("INPUT", { type: "number" })), false);

  assert.equal(isTextEditableElement(mkEl("INPUT", { type: "search" })), true);
  assert.equal(isTextEditableElement(mkEl("INPUT", { type: "number" })), true);
  assert.equal(isTextEditableElement(mkEl("TEXTAREA")), true);
  assert.equal(isTextEditableElement(mkEl("INPUT", { type: "range" })), false);
  assert.equal(isTextEditableElement(mkEl("BUTTON")), false);
});

test("Quick Actions gaze tracks cursor expressively while staying inside Ink silhouette", () => {
  const center = computeQuickActionsGaze(0, 0);
  assert.equal(center.eyeX, 0);
  assert.equal(center.eyeY, 0);
  assert.equal(center.eyeSquashX, 1);

  const farLeft = computeQuickActionsGaze(-400, 0);
  assert.ok(farLeft.eyeX < -28 && farLeft.eyeX >= -32, `left gaze=${farLeft.eyeX}`);
  assert.ok(farLeft.bodyX < -4 && farLeft.bodyTilt < -3);
  assert.ok(farLeft.eyeSquashX < 0.9);

  const farRight = computeQuickActionsGaze(400, 0);
  assert.ok(farRight.eyeX > 19 && farRight.eyeX <= 22, `right gaze=${farRight.eyeX}`);

  const farUp = computeQuickActionsGaze(0, -400);
  assert.ok(farUp.eyeY < -15 && farUp.eyeY >= -17, `up gaze=${farUp.eyeY}`);

  const farDown = computeQuickActionsGaze(0, 400);
  assert.ok(farDown.eyeY > 8 && farDown.eyeY <= 9.5, `down gaze=${farDown.eyeY}`);
});

test("sidebar trinket hops to varied keys, depresses one cap at contact, accelerates on burst, and resets after pause", () => {
  // Deterministic alternating choices so hops cover all three keycaps
  const picks = [0.2, 0.8, 0.2, 0.8, 0.2, 0.8, 0.2, 0.8];
  let pickIdx = 0;
  const ctrl = createSidebarTrinketController({
    random: () => picks[pickIdx++ % picks.length],
  });

  // Starts idle on the middle keycap (index 1, x=96)
  const initial = ctrl.buildRestingFrame();
  assert.equal(initial.currentKey, 1);
  assert.equal(initial.cat.x, TRINKET_KEY_POSITIONS[1]);
  assert.equal(initial.cat.y, TRINKET_PERCH_Y);
  assert.equal(initial.keys.every((k) => k.travel === 0 && k.lightOpacity === 0), true);

  // Single keystroke triggers crouch -> airborne arc -> contact depression of exactly one target key
  assert.equal(ctrl.triggerKeystroke(1000), true);
  assert.equal(ctrl.getState(1000).speed, 1);

  const crouch = ctrl.step(1020);
  assert.equal(crouch.hopping, true);
  assert.ok(crouch.cat.squash < 1, "crouch should squash before launch");
  assert.equal(crouch.keys.every((k) => k.travel === 0), true);

  const airborne = ctrl.step(1150);
  assert.ok(airborne.cat.y < TRINKET_PERCH_Y - 15, "cat should rise into hop arc");
  assert.equal(airborne.keys.every((k) => k.travel === 0), true);

  const contact = ctrl.step(1300);
  assert.equal(contact.expression, "land");
  assert.notEqual(contact.activeKeyIndex, 1, "must land on one of the other two keycaps");
  const litKeys = contact.keys.filter((k) => k.travel > 0 && k.lightOpacity > 0);
  assert.equal(litKeys.length, 1, "only target landing cap should depress and highlight");
  assert.ok(contact.cat.y > TRINKET_PERCH_Y, "cat baseline should move down with depressed cap");
  assert.ok(contact.impact.opacity > 0, "impact marks should flare at contact");

  // Settle cleanly back to resting on the landed keycap
  const settled = ctrl.step(1600);
  assert.equal(settled.hopping, false);
  assert.equal(settled.currentKey, contact.activeKeyIndex);
  assert.equal(settled.keys.every((k) => k.travel === 0 && k.lightOpacity === 0), true);

  // Sustained rapid burst accelerates speed toward ~2.45x and queues at most one pending hop
  let now = 2400;
  const landedKeys = new Set([settled.currentKey]);
  for (let i = 0; i < 8; i++) {
    ctrl.triggerKeystroke(now);
    now += 45;
  }
  const fastState = ctrl.getState(now);
  assert.ok(fastState.speed > 2.2 && fastState.speed <= 2.45, `fast speed=${fastState.speed}`);
  assert.equal(fastState.pendingHop, true, "burst should keep one pending hop queued");

  // Finish active hop + single pending hop; stopping typing leaves at most one follow-up
  let hopCompletions = 0;
  let prevKey = ctrl.getState(now).currentKey;
  for (let i = 0; i < 60; i++) {
    now += 20;
    const frame = ctrl.step(now);
    if (frame.currentKey !== prevKey) {
      hopCompletions += 1;
      prevKey = frame.currentKey;
      landedKeys.add(frame.currentKey);
    }
    if (!frame.hopping) break;
  }
  assert.equal(hopCompletions, 2, "active hop + at most one pending follow-up hop");
  assert.ok(landedKeys.size >= 2, "hops should vary target keycaps");

  // After > 720 ms pause, cadence clears back to normal 1x pace
  const afterPause = now + TRINKET_CADENCE_RESET_MS + 100;
  ctrl.triggerKeystroke(afterPause);
  assert.equal(ctrl.getState(afterPause).speed, 1);
});

test("sidebar trinket honors Animate Thocci off, reduced motion, and suspension reset", () => {
  const ctrl = createSidebarTrinketController({ random: () => 0.1 });

  // Reduced motion: short press highlight with zero airborne travel and zero impact marks
  assert.equal(ctrl.setReducedMotion(true), true);
  assert.equal(ctrl.triggerKeystroke(100), true);
  const rmContact = ctrl.step(130);
  assert.equal(rmContact.hopping, true);
  assert.ok(rmContact.cat.y >= TRINKET_PERCH_Y, "reduced motion must not lift airborne");
  assert.equal(rmContact.cat.stretch, 1);
  assert.equal(rmContact.cat.squash, 1);
  assert.equal(rmContact.impact.opacity, 0, "reduced motion must hide impact particles");
  assert.equal(rmContact.keys.filter((k) => k.lightOpacity > 0).length, 1);

  // Suspension clears pending hop, pressed cap, and cadence immediately
  ctrl.triggerKeystroke(150); // queues pendingHop
  assert.equal(ctrl.getState(150).pendingHop, true);
  assert.equal(ctrl.setSuspended(true), true);
  assert.equal(ctrl.setSuspended(true), false); // idempotent
  const susp = ctrl.getState(160);
  assert.equal(susp.hopping, false);
  assert.equal(susp.pendingHop, false);
  assert.equal(susp.speed, 1);
  assert.equal(ctrl.triggerKeystroke(200), false, "ignored while suspended");
  const resting = ctrl.buildRestingFrame();
  assert.equal(resting.keys.every((k) => k.travel === 0 && k.lightOpacity === 0), true);

  // Animate Thocci off: static resting pose on middle key, ignores keystrokes
  assert.equal(ctrl.setSuspended(false), true);
  assert.equal(ctrl.setAnimated(false), true);
  assert.equal(ctrl.setAnimated(false), false); // idempotent
  assert.equal(ctrl.getState().currentKey, 1);
  assert.equal(ctrl.triggerKeystroke(500), false, "ignored while Animate Thocci is off");
});
