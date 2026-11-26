# Thocci desktop assets

Use these resources to implement the first-launch welcome, Quick Actions, desktop dashboard, and soundpack browser. `desktopui.md` at the repository root defines behavior and platform integration. Use the design canvases to check visual alignment. [Thocci concept UI](https://www.figma.com/design/IPLcvUwDDv7VUCnqO89fht/Thocci---concept-UI?node-id=3-235&p=f)

| Location | Contents and use |
| --- | --- |
| `brand/` | The owner-supplied ESC logo, a 1024 × 1024 square rendition, native Windows ICO and macOS ICNS files, and PNG sizes for Linux and Tauri. |
| `brand/tray/` | Transparent monochrome Thocci silhouettes in SVG and 16, 22, and 32 px PNG. Use the black template with macOS template rendering. On other hosts, choose the dark or light mark for the tray surface and scale. |
| `mascot/` | Editable open, excited, and sleeping expressions. Place the open mascot behind the Quick Actions deck and animate its `thocci-eyes` group for bounded pointer-follow. Use `stage-poses.json` for stage-relative placement and layer the optional motion accents using the listed positions. |
| `illustrations/` | The onboarding illustration and separate light and charcoal stage backgrounds. Layer a live mascot over the stage so the pose can change independently. |
| `icons/` | Solid rounded UI icons for the tray popover, dashboard, and pack browser. Inline the SVG or use it as a mask to apply a token color. Give interactive icon controls accessible names. |
| `keyboard/` | The 79-key Portuguese ISO layout, face paths, legend positions, and hardware glyphs. Render each key independently and match presses by physical `code`. |
| `fonts/` | DM Sans Bold and variable Inter for offline use, with their SIL Open Font Licenses. |
| `tokens.json` | The approved layout dimensions, theme palette, typography, Quick Actions geometry, and soundpack sidebar timings. |

## App icon and platform packaging

- Keep `brand/thocci-app-icon-source.png` as the original. The square `brand/thocci-app-icon.png` is an ImageGen rendition from that source. The edit prompt requested square off-white framing and preservation of the black mascot, titanium ESC keycap, and white lettering.
- Package `brand/thocci-app-icon.icns` for macOS, `brand/thocci-app-icon.ico` for Windows, and the appropriate files in `brand/png/` for Linux desktop entries and packages. Add those files to Tauri’s bundle icon configuration when integrating the UI.
- Keep the tray mark separate from the full app icon so Thocci’s face is recognizable at small sizes.

## Rendering and motion

- For pointer-follow, inline `mascot/thocci-quick-actions.svg` and apply a bounded transform to its eyes. The SVG uses a 160 × 119 coordinate space. The stage clips and scales poses within its 850 × 304 design space. Tint the slide trail and sleep marks with the stage orbit color. Tint the jump shadow with a dark neutral.
- In the keyboard layout, the key bounds are relative to the keyboard origin. Face paths and legend anchors are relative to each key. Legend `y` values mark vertical centers. For SVG text using an alphabetic baseline, the reference adds `size × 0.33` to that anchor. Preserve the L-shaped Return and the two separate half-height arrow keys. Resolve glyph paths relative to `keyboard/`.
- Use the motion values from `tokens.json` for the sidebar. Keep the stage and keyboard parked during the transition. Use the handoff for focus management, key audition, stage reactions, and reduced motion.
- Serve `fonts/fonts.css` with the bundled WOFF2 files and accompanying licenses. Use a system font fallback for keyboard legends outside the font’s coverage. [DM Sans](https://github.com/google/fonts/tree/main/ofl/dmsans) · [Inter 4.1](https://github.com/rsms/inter/releases/tag/v4.1)
- Load the bundled audio from the existing `assets/soundpacks/` through the Rust catalog. Use the active runtime pack for dashboard and Quick Actions auditions.
