# Thocci dashboard animation handoff

Replace the dashboard mascot's pose cycle with the behavior in the [approved interactive preview](assets/desktop/animation-reference/thocci-blob-preview.html). Open it in a browser, type one key, type steadily for a few lines, try a burst of fast typing, then stop. The deliverable is the new mascot in the existing Tauri dashboard.

Keep Ink's original 160 × 119 black blob silhouette, uneven ears, and simple white eyes. Retain the dashboard's stage dimensions, rounded clipping, and theme behavior. Use a quiet stage backdrop and a small monochrome pillow in place of the concentric ripples. The preview's title, status label, and typing field are demo controls.

## Project entry points

- [`ui/src/stage.js`](ui/src/stage.js) mounts the current stage. Replace its timer-driven `REACTION_SEQUENCE` and per-pose SVG replacement with a persistent SVG rig. Preserve the dashboard-facing `mountMascotStage`, `reactToKeystroke`, `setAnimated`, and `destroy` behavior. `setPose` has no dashboard caller in the current tree. Check for consumers before changing it.
- [`ui/src/dashboard.js`](ui/src/dashboard.js) connects the mascot to the visual keyboard activity callback, saves the Animate Thocci preference, applies the app theme, and swaps the stage with the Trivia Typing Challenge. Use that wiring for the new controller. Keep keyboard audio, physical key highlighting, on-screen audition, the flying typed characters, the challenge, and Quick Actions working as before.
- [`ui/src/keyboard.js`](ui/src/keyboard.js) already emits one activity event for supported physical presses and on-screen key presses and ignores physical key repeat. Use that callback as the animation's input. The preview's document-level listeners and textbox are only for running the reference in a browser. The mascot needs event timing alone.
- [`ui/styles/app.css`](ui/styles/app.css) owns the stage slot, challenge transitions, and current .stage-mascot-layer transform transition. Remove that transition from the frame-driven rig so it can follow the animation loop directly. Put new theme colors in [`ui/styles/tokens.css`](ui/styles/tokens.css). Make the stage follow the app's current light or dark selection, including switches while Thocci is active.

## Artwork and packaging

| Asset | How to use it |
| --- | --- |
| [`thocci-blob-preview.html`](assets/desktop/animation-reference/thocci-blob-preview.html) | Standalone reference for pacing, easing, corner routes, eye layers, bubble placement, and the phrase list. Open locally. Port its stage behavior into the app's modules and styles. |
| [`thocci-quick-actions.svg`](assets/desktop/mascot/thocci-quick-actions.svg) | Use its exact body path and open eye geometry for the dashboard rig. The shared Quick Actions art stays in place. |
| [`thocci-sleeping.svg`](assets/desktop/mascot/thocci-sleeping.svg) and [`thocci-excited.svg`](assets/desktop/mascot/thocci-excited.svg) | Reuse the existing closed-eye curves and excited chevrons as face layers on the same body. The preview's `data-shocked` group supplies the two round shocked eyes. Use individual eye scaling and a small eye-group offset for the curious look and wink. |
| [`thocci-pillow.svg`](assets/desktop/mascot/thocci-pillow.svg) | New pillow artwork extracted from the preview. Keep it centered beneath the sleeping mascot. Its viewBox uses a pillow-local origin. Inline its vector geometry, or load and insert it inline so the stage's `--stage-pillow-*` tokens and their fallbacks apply. |

[`scripts/prepare-ui.mjs`](scripts/prepare-ui.mjs) copies `assets/desktop/mascot/` into generated `ui/vendor/mascot/` via `npm run build:ui`. Use the existing asset loader for bundled SVGs and add the pillow to the build's required files if the stage loads it at runtime. The reference directory is outside the copied asset folders. Tauri serves `ui/` and its CSP requires scripts from the app origin, so put animation logic in the existing JavaScript modules. Preserve legacy pose files and their checks if they serve as design references. If the new controller supersedes obsolete stage implementation details, keep that cleanup scoped to the dashboard mascot.

## Motion and expressions

- **Sleeping by default.** Start with Ink on the pillow at roughly the center of the 850 × 304 stage and a body baseline near y = 245. Use subtle breathing, closed eyes, and three staggered Zs floating upward beside him.
- **Wake on the first key.** Open the eyes promptly, give the body a small stretch, and fade the Zs. Sparse keys keep Ink nearby with occasional blinks and a small twitch. A pointer press on an on-screen key is equivalent to a physical key. A click on the stage itself leaves a sleeping Ink at rest.
- **Build energy from cadence.** Use recent activity and decay instead of mapping each press to a pose. The reference uses an energy decay constant of about 670 ms. A brief burst of sustained typing starts play. Shuffle pouncing, investigating with a head tilt, double hops, zooming, and corner peeks without repeating the previous play action at the bag boundary. Seed the first round with corner peeks and a zoom so the behavior is easy to discover.
- **Peek from all four corners.** Move diagonally offstage or partly outside the rounded edge, then ease into view with both eyes visible and the body partially clipped. Vary the eye expression and tilt by corner. The reference pairs top-left with a curious squint, bottom-right with excited chevrons, top-right with shocked circles, and bottom-left with a wink. Visit two corners per peek sequence and cycle across all four over successive sequences. Let the expression hold briefly before the next move.
- **Use visible depth.** Recede to roughly half-size, rush toward the viewer to roughly 1.6×, then settle back. Scale the shadow with depth and use a gentle lean, squash and stretch, and changing expressions through the pass. Keep body, face, and shadow aligned.
- **React to very fast typing.** The reference detects about nine presses in 960 ms. Brake smoothly, flash round shocked eyes, and show “Hey slow down a bit!” on the first fast burst. Rotate the other warning phrases on later bursts. Warn once per burst and let this reaction interrupt ordinary chatter.
- **Slow down and return home.** Let the current movement ease into a return when activity fades. Retain the current position and momentum when redirecting. Settle onto the pillow, close the eyes, and bring back the Zs. Fresh activity during the return can wake Ink or redirect him smoothly.

Mount the body, eyes, pillow, effects, and bubble once. Drive transforms and expression blending from a single `requestAnimationFrame` loop with elapsed time. Use bottom anchoring when Ink squashes or scales. Keep the existing rounded stage clipping, and check both upper and lower corners after resizing. The preview is the reference for timing and feel. Tune values only as needed for the real stage and its WebView.

## Speech and lifecycle

- Use the short cues and phrase rotation in the preview. Ordinary speech should be occasional, with roughly 2 seconds of dwell and at least 3 seconds between lines. Keep urgency separate so a speed warning can interrupt a greeting. Update the bubble text once per cue and expose it through a polite live region.
- Anchor the bubble to Ink, clamp it within the stage, and place it inward at the upper corners. Hide it when Ink slips out of view. Keep it pointer-transparent, readable in both themes, and clear of the flying-character row during normal use.
- The Animate Thocci switch remains the source of truth. Off shows a static sleeping pose and stops reactions, bubbles, and motion. With reduced motion enabled, keep Ink on the pillow and allow brief expression and speech feedback without travel, zoom, bobbing, or drifting Zs.
- Suspend when the document is hidden or the stage is replaced by the challenge. Resume with fresh timing and no queued reactions. Clean up animation frames, visibility and media-query subscriptions, resize tracking, and any timers in `destroy()`. Keep preference reconciliation idempotent so focus and dashboard-show events cannot restart a running loop or duplicate it.

## Verification

1. Run `npm run build:ui`, `npm run test:ui`, and `git diff --check`. Verify that any runtime asset added under `assets/desktop/mascot/` is available under `ui/vendor/mascot/` after preparation.
2. Run `npm run dev` and check the actual dashboard. On launch Ink should sleep. Try a single physical key, an on-screen key, deliberate slow typing, steady typing through all four corners and a full zoom, a fast burst, and a pause. Compare against the local HTML reference.
3. Check the narrowest supported dashboard width and both app themes. Verify corner clipping leaves both eyes visible, speech stays inside the stage, and the bubble and flying characters remain readable. Check typing again while Ink is returning to the pillow.
4. Toggle Animate Thocci while Ink moves, enable reduced motion, hide and reopen the dashboard, and enter and exit the challenge. Verify audio and key feedback still work, the challenge has no mascot reactions, and there are no stale bubbles or accumulating animation loops.
5. Report the checks run and any native WebView or platform limitations that remain. If the native dashboard cannot be opened, exercise the mounted stage in a browser and identify the desktop checks that were blocked.
