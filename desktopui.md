# Thocci Desktop UI implementation handoff

This document tells an implementation agent how to turn the approved Thocci Figma design into a working desktop app for macOS, Linux, and Windows. It covers first launch, the menu bar and system tray, the dashboard, the soundpack browser, keyboard feedback, mascot motion, and the connection to Thocci’s existing Rust audio engine.

**Thocci** is the name of both the app and its black, cat-eared mascot. Use **Thocci** in app copy, accessible names, setup guides, and new asset names. The repository’s older artwork and README refer to the mascot as Ink.

## Design references

Use the Figma file for layout, proportions, artwork, and visual states. Use this document for behavior, integration, and platform fallbacks.

| Surface | Reference |
| --- | --- |
| Complete project and Desktop UI canvas | [Thocci concept UI](https://www.figma.com/design/IPLcvUwDDv7VUCnqO89fht/Thocci---concept-UI?node-id=3-235&p=f) |
| First-launch welcome | [Onboarding](https://www.figma.com/design/IPLcvUwDDv7VUCnqO89fht/Thocci---concept-UI?node-id=3-2) |
| Enabled and disabled popover states | [Quick Actions](https://www.figma.com/design/IPLcvUwDDv7VUCnqO89fht/Thocci---concept-UI?node-id=3-37) |
| Desktop dashboard | [Home — Light](https://www.figma.com/design/IPLcvUwDDv7VUCnqO89fht/Thocci---concept-UI?node-id=3-1083) · [Home — Dark](https://www.figma.com/design/IPLcvUwDDv7VUCnqO89fht/Thocci---concept-UI?node-id=3-1485) |
| Expanded soundpack sidebar | [Soundpacks — Dark](https://www.figma.com/design/IPLcvUwDDv7VUCnqO89fht/Thocci---concept-UI?node-id=3-5482) · **Soundpacks - Light** on the Desktop UI canvas |
| Mascot expressions and poses | **Thocci poses** on the Desktop UI canvas |

## Bundled desktop assets

Use `assets/desktop/README.md` in the Thocci repository for the asset inventory and integration notes. The kit includes the official ESC app icon and platform exports, a monochrome tray mark, the layered Quick Actions mascot, stage expressions and motion accents, the onboarding and stage illustrations, the filled icon set, the Portuguese ISO keyboard geometry, the offline fonts, and the design tokens. Use the existing `assets/soundpacks/` and Rust catalog for audio.

## Start from the existing code

Repository baseline inspected at commit `43836d2`. Recheck the current branch before editing. Preserve the CLI as a supported way to use the same engine. Keep the GUI and CLI on one daemon, one pack selection, and one volume setting.

| Area | Existing behavior and implementation work |
| --- | --- |
| [`ui/index.html`](https://github.com/ItzEvhyx/Thocci/blob/43836d2/ui/index.html) and [`src-tauri/tauri.conf.json`](https://github.com/ItzEvhyx/Thocci/blob/43836d2/src-tauri/tauri.conf.json) | The Tauri 2 shell serves a minimal HTML UI in a resizable 440 × 460 window. Build the approved shared frontend and configure sizes appropriate to each surface. |
| [`src-tauri/src/lib.rs`](https://github.com/ItzEvhyx/Thocci/blob/43836d2/src-tauri/src/lib.rs) | Reuse `get_status`, `list_packs`, `start_daemon`, `stop_daemon`, `toggle_daemon`, `set_volume`, and `set_pack`. Add the tray, window routing, preview playback, and custom pack support here or in narrowly scoped modules. Run blocking daemon and decoding work off the UI thread. |
| [`client.rs`](https://github.com/ItzEvhyx/Thocci/blob/43836d2/crates/thocci-core/src/client.rs) and [`daemon.rs`](https://github.com/ItzEvhyx/Thocci/blob/43836d2/crates/thocci-core/src/daemon.rs) | The client starts and stops the engine and shares its state with the CLI. A live PID can outlast a failed status query, so distinguish a responsive daemon from a starting or unavailable one in the UI. The current live pack command accepts bundled IDs. |
| [`soundpack.rs`](https://github.com/ItzEvhyx/Thocci/blob/43836d2/crates/thocci-core/src/soundpack.rs) and [`paths.rs`](https://github.com/ItzEvhyx/Thocci/blob/43836d2/crates/thocci-core/src/paths.rs) | Reuse the WAV decoder, per-key lookup, press and release clips, and custom directory loader. Saved pack IDs currently accept bundled packs only. Extend pack resolution and persistence together so imports work across restart and live switching. |
| [`install.sh`](https://github.com/ItzEvhyx/Thocci/blob/43836d2/install.sh), [`install.ps1`](https://github.com/ItzEvhyx/Thocci/blob/43836d2/install.ps1), and [release workflow](https://github.com/ItzEvhyx/Thocci/blob/43836d2/.github/workflows/release.yml) | The release workflow currently packages the CLI binary. The scripts finish with the terminal `--welcome`. Add desktop release assets and connect the scripts to the graphical first-launch flow while preserving the CLI installation path. |

Keep UI preferences such as theme, mascot animation, and onboarding completion separate from daemon settings. Reconcile live status after a GUI action, when the dashboard regains focus, and when the popover opens. Serialize mutations so rapid clicks, volume drags, or parallel tray and CLI changes converge on the engine’s confirmed state. Distinguish a saved setting from a live change that failed. Leave the last working pack available until a replacement loads successfully.

## Install and first launch

1. Keep the macOS and Linux shell-command installation and the Windows PowerShell installation documented in the [repository README](https://github.com/ItzEvhyx/Thocci/blob/43836d2/README.md#installation). Install or locate the GUI bundle for the matching platform and architecture alongside the CLI. Launch the GUI through the installed app entry point once the desktop install succeeds in a graphical session. Fall back to the terminal welcome for CLI-only or headless installations. Keep existing CLI entry points and release asset compatibility.
2. Show the Figma welcome on the first successful graphical launch. Use the **Welcome to Thocci** headline and the exact tagline **“Thocci - the keyboard you can hear.”** Place the mascot on the black spacebar, with the pale keycaps and keystroke ripples from the illustration. Use a single **Get started** action.
3. **Get started** opens the dashboard, completes onboarding for that user, and makes Quick Actions available when the host supports it. Subsequent launches open or focus the existing dashboard. Preserve explicit playback settings across upgrades and avoid changing keyboard access automatically.
4. Explain any permission required for background playback in the setup guide or when the user enables Thocci. Keep the dashboard, soundpack browser, and local pointer-driven audition accessible when background permissions are pending or denied.

## Quick Actions

Build one compact popover, approximately **288 × 282 logical px**, with an OS-specific tray or menu-bar host. Place **Thocci** at the top of the white rounded shell. The black mascot peeks above and visually joins the black control deck. Use low-profile white keycaps with cool gray edges and solid gray icons.

| Control | Behavior |
| --- | --- |
| Left keyboard keycap | Enable or disable background sounds. Show **Thocci is on** or **Thocci is off** beside the keycap. The off state uses the slashed keyboard icon. Update the status after the engine confirms the transition. Keep the popover open so the result is visible. |
| Right gear keycap | Open or focus the dashboard, then dismiss the popover. Use the filled gear as its only visible label and provide a **Settings** accessible name. |
| Bottom **S W I T C H E S** row | Render eight separate letter keycaps. Clicking any letter triggers the same action, opening the dashboard directly to its Soundpacks sidebar. Give the row one keyboard-focusable **Browse soundpacks** action. The specific cap under the pointer still gets its own pressed feedback. |

On each successful activation, depress or highlight the triggered keycap and play one short sample from the currently active soundpack at the current volume. Use the matching sample when the pack provides it and a generic or representative key sample for icons. Keep this intentional click audition available in the off state, including when the left key disables the engine. Volume at 0 stays silent. Keep ordinary background typing silent when off. Treat the popover as a GUI preview client so an activation can make one sound without starting a second listener or producing duplicate audio.

When the panel opens, let Thocci’s eyes gently follow the pointer inside the popover. Clamp gaze movement to the mascot’s face, interpolate smoothly, return to neutral when the pointer leaves, and stop tracking when hidden. Respect reduced-motion preferences and show a stable expression.

Open or close the panel from the tray button. Dismiss it on Escape, outside click, or loss of focus where the host supports that behavior. Keep focus within the panel while it is in use and return it appropriately when closing. Check all placements on the tray’s actual monitor and clamp within its usable screen area.

## Desktop dashboard

Build a resizable two-column desktop window. The Figma reference is **1160 × 806** with a **246 px** sidebar, an **850 × 304** mascot stage, and an **850 × 349** keyboard. Treat these as logical design references. Preserve the layout and keyboard aspect ratio as available space changes. Allow scrolling at constrained sizes and keep window controls and sidebar actions reachable.

| Sidebar group | Rows and actions |
| --- | --- |
| **Playback** | **Enable Thocci** mirrors Quick Actions and the daemon. **Volume** is a labeled 0–100 slider with a visible percentage, keyboard adjustment, immediate feedback, and coalesced live updates. |
| **Soundpacks** | **Browse soundpacks** shows the active pack as its secondary line. **Import WAV** opens the native file picker, validates the file, and adds the imported pack to the library. |
| **App** | **Animate Thocci** enables or pauses stage motion. **Setup guide** opens concise instructions for enabling playback, platform permissions, changing packs, importing audio, and recovery from common errors. |

Place the light/dark toggle at the top right. Use a filled moon with **Dark mode** in light mode and a filled sun with **Light mode** in dark mode. Initialize from the OS appearance when there is no saved preference, then persist the user’s explicit choice. Preserve the active pack, volume, current sidebar view, and any search or selection state when switching themes.

### Interactive keyboard

- Recreate the approved **79-key Portuguese ISO Magic Keyboard layout** as individual interactive keycaps. Match the keyboard geometry and legends in Figma, including the tall L-shaped Return, Ç, the extra ISO key beside left Shift, split Up and Down, the bottom-row modifiers, the function row, and the media legends. Preserve the white key faces and silver chassis in both themes.
- Match physical positions using `KeyboardEvent.code` in the focused webview and Thocci’s existing evdev-normalized mapping in Rust. Keep the Portuguese ISO legends in the approved visual layout. Layout translations and modifiers must still light the corresponding physical-position key. Treat OS-reserved keys and unmatched keys gracefully.
- On physical keydown, accent and visually depress the matching cap. On keyup, restore it. Track held keys independently so chords work. Clear held states on blur, hide, suspend, or listener disconnect. Respect existing repeat and ignored-key policies for sound playback.
- Let the user click or tap any rendered key to play the active pack’s sample and see pressed and released states. Release pointer-held keys on pointer cancellation as well as normal release. Play the release sample when available.
- When global playback is active, physical keystroke sound comes from the daemon and the focused webview supplies the visual state. Avoid a second sound for the same event. The handoff default is that physical keys still highlight while Thocci is off and explicit pointer auditions still play. Keep ordinary typing and text editing in fields such as Search soundpacks available without taking focus or invoking dashboard shortcuts.

### Thocci’s stage

Reserve the area above the keyboard for Thocci. Use the editable mascot from Figma and its two white eyes. Keep the shape recognizable and use the stage mask so Thocci can enter from the stage edges.

- Include **peek up from the bottom**, **peek from the side**, **slide**, **jump**, and **sleep**. Use open, excited, and closed-eye expressions where appropriate. Use the Figma poses for crops and proportions.
- Start from the bottom peek. Use short responses to typing activity, vary reactions, and let Thocci settle to sleep after idle time. On the next interaction, wake Thocci and return toward the resting pose. Coalesce rapid keystrokes so animation remains smooth at typing speed.
- Pause motion when the dashboard is hidden, the animation toggle is off, or the session is suspended. Respect reduced motion with a still pose or a brief expression change. Animation state must be independent of the audio engine’s playback and timing.

## Soundpack browser and WAV import

- The browser is another view **inside the sidebar**. Keep the dashboard heading, Thocci stage, keyboard, and window geometry fixed. Build a **Soundpacks** header with a rounded Back arrow, a search field, a scrollable list, the pack count, and an active-pack checkmark. Populate the list using the core catalog so the names and number stay in sync with the installed build. The reference shows 20 built-ins and Creamy selected as an example. Use the real active pack at runtime.
- On **Browse soundpacks**, clip both panes within the sidebar. Translate settings from 0 to minus one sidebar width and the incoming pack list from plus one width to 0. Use **320 ms** and **`cubic-bezier(0.55, 0.03, 0.86, 0.50)`**. This is the approved slow-start, accelerating left slide. Use transforms and keep the workspace at rest.
- On **Back** or Escape within the browser, reverse the direction with a **260 ms ease-out**. Allow interruption without jumping or leaving both panes interactive. Under reduced motion, swap panes immediately and preserve focus. Focus the new view on entry and restore the Browse soundpacks control on exit.
- Filter by pack name and available descriptive metadata. Handle no results, clear search, and keep the active selection visible wherever it matches. Clicking a row or activating it with the keyboard selects the pack and updates the checkmark after confirmation. Preserve the browser state when changing theme.
- Use **Import WAV** to pick a local `.wav` file. The handoff default is **one WAV becomes one reusable generic key-press soundpack**. Validate by decoding the contents and enforce a bounded clip size and duration. Copy accepted audio into the per-user pack directory under an app-generated ID. Reuse the existing v2 pack format with a `Generic` fallback and one press range covering the decoded clip. Import through the existing format so more detailed packs can continue to have distinct per-key and release sounds.
- Show usable feedback for unreadable, empty, unsupported, or failed imports. Persist only complete, valid imports and preserve the current working pack if a load or switch fails. Resolve bundled IDs and imported IDs in the shared engine and CLI path, including restart and live switching. Keep existing custom directory support and bundled pack IDs compatible. Local file import stays on the device.

## Visual rules

Use the approved rounded, mostly solid surfaces with a fine top and side rim highlight, soft depth, and restrained shadows. Pull illustrations, icon vectors, and detailed proportions from Figma. Use shared semantic tokens so dashboard, popover, and sidebars remain consistent.

| Role | Reference values and behavior |
| --- | --- |
| Typography | **DM Sans Bold** for the wordmark and display headings. **Inter** for controls, status, supporting text, and legends, with a system UI fallback. Keep font metrics and hierarchy consistent across OSes. Dashboard title is roughly 32 px, sidebar wordmark 26 px, rows 14 px, supporting copy 12–14 px, and group headings 11 px. |
| Light | Window `#FDFDFF`, sidebar `#F3F5FA`, primary text `#17181D`, secondary text `#575B67`, accent `#435EC6`. |
| Dark | Near-black canvas `#080809`, window `#111112`, sidebar `#181819`, rows `#212122`, dividers `#313133`, primary text `#F5F5F5`, secondary text `#B0B0B2`, neutral control accent `#D4D4D6`. Use charcoal for stage lighting and pressed states. Keep dark-mode accents neutral gray. |
| Keyboard | Silver chassis around `#C9CDD3`, white keycaps around `#FAFAFB`, gray legends around `#50545D`. Show an unambiguous pressed state in both themes. |
| Quick Actions | White shell, black deck around `#17181B`, white keycap faces with soft gray edges, black mascot with white eyes. Keep the approved appearance across host platforms. |
| Icons | Reuse the approved **solid, rounded, filled** SVG set for the keyboard toggle, slashed keyboard, gear, theme, sidebar, Back, search, and confirmation. Keep the physical function-row glyphs from the keyboard reference. Supply accessible labels for icon-only controls. |

Use semantic buttons, a labeled search field, clear focus rings, and selection states that communicate more than color. Keep the **SWITCHES** row a single logical action. Make smaller icon controls easy to target and preserve keyboard navigation in the tray popup and dashboard.

## Platform integration

Build shared front-end components and keep the native differences in a small host layer. Use the installed Tauri version and its tray/window APIs as the entry point. Add individually scoped capabilities for the dashboard and popover and package local assets with them. Use platform-specific hosting where tray events or placement differ. [Tauri tray guide](https://v2.tauri.app/learn/system-tray/) · [Tray events for the pinned Tauri version](https://docs.rs/tauri/2.12.7/tauri/tray/enum.TrayIconEvent.html) · [Tauri window API](https://v2.tauri.app/reference/javascript/api/namespacewindow/#setposition)

| Host | Implementation and fallback |
| --- | --- |
| **macOS** | Anchor Quick Actions below the menu-bar item on its current display. Use the tray anchor when available or an AppKit status-item/popover bridge if needed. For background input, provide steps for **System Settings → Privacy & Security → Input Monitoring** for the actual installed capture process. Refresh state after the user returns and explain when relaunch is necessary. Test Intel and Apple silicon builds supported by the existing matrix. [AppKit status item](https://developer.apple.com/documentation/appkit/nsstatusitem) · [AppKit popover](https://developer.apple.com/documentation/appkit/nspopover) · [Input Monitoring](https://support.apple.com/en-bn/guide/mac-help/mchl4cedafb6/mac) |
| **Linux** | Open below a top panel when a supported tray host and placement signal are available. The Tauri tray event API does not emit icon-click events on Linux, so provide an indicator menu with equivalent actions and a desktop launcher. A compositor or panel adapter can supply the custom anchored popover where supported. On Wayland, use a compositor-supported popup or a normal compact Quick Actions window when a tray anchor is unavailable. Keep a launcher entry point when the desktop has no tray. Use the existing evdev input backend and guide the user through the repo’s documented access steps when needed. Test X11 and Wayland separately. [Tauri Linux tray behavior](https://docs.rs/tauri/2.12.7/tauri/tray/enum.TrayIconEvent.html#platform-specific) · [Wayland popup protocol](https://wayland.app/protocols/xdg-shell) |
| **Windows** | Start from the taskbar notification-area icon. Anchor the popover above a bottom taskbar and adapt to the actual work area for secondary displays or alternate placement. Use tray event geometry and DPI-scaled coordinates, with `Shell_NotifyIconGetRect` as a native option when needed. Test tray overflow, taskbar restart, and 100%, 125%, and 150% scaling. Use the existing interactive-session keyboard hook and report startup failures through the UI. [Windows notification area](https://learn.microsoft.com/en-us/windows/win32/shell/notification-area) · [Icon geometry](https://learn.microsoft.com/en-us/windows/win32/api/shellapi/nf-shellapi-shell_notifyicongetrect) |

Keep Quick Actions, gear routing, and pack selection available through the main window when any platform tray feature is unavailable. Reuse or focus the same dashboard from the launcher, gear, and **SWITCHES** row. While hidden, the dashboard should consume no animation work. Keep background playback governed by the explicit playback toggle. Provide a native Quit route and avoid leaving an orphan capture process on explicit quit.

Process key events locally and discard them after playback or visual feedback. Avoid logging captured text, key histories, global pointer movements, or imported audio contents. Restrict any focused-window or daemon event bridge to the transient state needed for the visible UI.

## Delivery and verification

Implement in this order.

1. Build the shared themed dashboard, keyboard, and soundpack sidebar against Figma. Verify static light and dark states and the 320 ms opening / 260 ms return at full and constrained sizes.
2. Connect the dashboard and Quick Actions to shared engine state, single-instance routing, volume, live pack switching, and explicit audition playback.
3. Add mascot states, reduced motion, and native popover hosting or platform fallbacks. Add import, setup guidance, and first-run state.
4. Package the Tauri desktop app for the current supported release targets. Connect the existing installers to the new desktop assets, with their CLI behavior still usable. Update installation and permission documentation with the actual shipped behavior.
5. Run the repository’s existing checks and build the desktop bundle. Verify installation and launch on real target systems. Record the combinations that were exercised and any remaining platform limitations.

```bash
npm install
npm run dev
npm run build
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

- The graphical package opens Welcome once and Get started opens the dashboard. Verify relaunch, update, CLI-only mode, and denied input permissions.
- In Quick Actions, test the keyboard icon, status copy, gear, and all eight letter keycaps. Each activation highlights the intended cap and plays one audition sample at the chosen volume. Test Escape, outside click, launch routing, and pointer-follow.
- On the dashboard, compare both themes and keyboard geometry to Figma. Verify physical press and release, chords, pointer audition, modifier mapping, mute, volume 0, and exactly one playback event per action. Exercise the stage poses and reduced motion.
- In Soundpacks, check the accelerating left slide with a stable main workspace. Test Back, search, empty results, theme switching, active-pack feedback, custom WAV errors, imported-pack selection and persistence, and synchronization with CLI changes.
- Cover macOS permissions and menu bar, Linux X11 and Wayland with and without a tray host, and Windows taskbar placement and scaling. Confirm the packaged GUI and CLI refer to the same playback state.
