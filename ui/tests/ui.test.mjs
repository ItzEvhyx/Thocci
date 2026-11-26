import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { KeyHoldTracker } from "../src/keyboard.js";
import { filterCatalog } from "../src/dashboard.js";
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
