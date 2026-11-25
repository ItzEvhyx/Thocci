# Thocci

Mechvibes-style mechanical keyboard sounds for **Linux**, **Windows**, and **macOS**, written in Rust with a Tauri desktop shell.

Every physical keystroke plays a low-latency clip from an embedded or custom Mechvibes soundpack. No Python, no `ffmpeg`, and no external asset folder required — built-in soundpacks are compiled directly into the standalone `thocci` binary.

## Installation

### Linux & macOS (Bash / Zsh / Fish)

```bash
curl -fsSL https://raw.githubusercontent.com/ItzEvhyx/Thocci/main/install.sh -o /tmp/thocci-install.sh && bash /tmp/thocci-install.sh
```

### Windows (PowerShell)

```powershell
irm https://raw.githubusercontent.com/ItzEvhyx/Thocci/main/install.ps1 | iex
```

Both installers place `thocci` in `~/.local/bin` (`%USERPROFILE%\.local\bin` on Windows) and verify the release archive checksum.

### Platform permissions

- **Linux**: Add your user to the `input` group once, then log out and back in:
  ```bash
  sudo usermod -aG input "$USER"
  ```
- **Windows**: Runs out of the box via a low-level keyboard hook. Note that elevated apps (some game launchers/admin terminals) only deliver hooks if Thocci is also run elevated.
- **macOS**: Grant **Input Monitoring** (or **Accessibility**) permission to your terminal or Thocci in *System Settings → Privacy & Security*.

## Commands

```text
thocci on                # start background daemon
thocci on --volume 60    # start at a specific volume (0-100)
thocci on --pack /path   # start with a custom Mechvibes pack directory
thocci off               # stop daemon
thocci toggle            # flip on/off
thocci status            # show running/stopped, volume, and active pack
thocci volume 50         # adjust volume live (0-100)
thocci cherry-mx         # switch to Cherry MX pack (default)
thocci banana-split      # switch to Banana Split pack
thocci packs             # list built-in soundpacks
thocci uninstall         # stop daemon and remove Thocci
```

## Built-in Soundpacks

All 20 soundpacks live in `assets/soundpacks/` and are embedded directly into `thocci` at build time (supporting both Mechvibes v1 and v2 press+release formats, `.ogg`, `.wav`, and `.mp3`):

| Command | Pack |
|---------|------|
| `thocci cherry-mx` | CherryMX Black - ABS keycaps (default) |
| `thocci banana-split` | Banana Split Lubed by Akira |
| `thocci pack aula-f75-leobog-reaper` | aula f75 max leobog reaper switch |
| `thocci pack clicky-perfect` | clicky perfect |
| `thocci pack creamy` | creamy (still getting better) |
| `thocci pack fortnite-keyboard` | fortnite keyboard |
| `thocci pack gravastar-v75` | GravaStar V75 (Ploszy ASMR rip) |
| `thocci pack hayyoths-keyboard` | Hayyoth's keyboard |
| `thocci pack hyperx-alloy-cherry-mx-blue` | Hyperx Alloy Cherry MX Blue |
| `thocci pack kailh-saker-mini` | CnT - Kailh Saker Mini Low Profile |
| `thocci pack keychron-dream-fn-v2` | Myilo Keychron Dream FN V2 |
| `thocci pack kiiboom-matcha-latte` | Presskey - KiiBOOM Matcha Latte |
| `thocci pack leobog-hi75-reaper` | LEOBOG Hi75 (Reaper Switches) |
| `thocci pack nuphy-halo75-v2-raspberry` | NuPhy Halo75 V2 (Raspberry Switches) |
| `thocci pack orange-deluxe` | Orange deluxe - Press + Release separated |
| `thocci pack satisfaction-bubblepop` | Satisfaction - Bubblepop |
| `thocci pack typewriter` | TypeWriter |
| `thocci pack wallhack-k-001` | Wallhack K-001 |
| `thocci pack womier-sk75-tmr` | womier sk75 tmr |
| `thocci pack wooden-keyboard` | Wooden Keyboard |

Run `thocci packs` to list all packs. Switching soundpacks and adjusting volume happen live while Thocci is running.

## Architecture

Thocci is organized as a Rust workspace so the CLI stays lightweight and headless while the Tauri desktop app shares the exact same engine:

```text
Thocci/
├── assets/soundpacks/ # 20 built-in Mechvibes v1 & v2 soundpacks (auto-embedded)
├── crates/
│   ├── thocci-core/   # Soundpack decoder (Symphonia), low-latency mixer (cpal),
│   │                  # OS key listeners (evdev / Win32 hook / CGEventTap),
│   │                  # daemon, localhost TCP control server & client
│   └── thocci-cli/    # Standalone `thocci` CLI + daemon binary
├── src-tauri/         # Tauri v2 desktop shell wired to `thocci-core`
├── ui/                # Minimal placeholder UI (desktop UI coming next)
├── install.sh         # One-line Linux/macOS installer
└── install.ps1        # One-line Windows PowerShell installer
```

### Runtime state & logs

- **Linux**: `~/.local/state/thocci/` (`$XDG_STATE_HOME/thocci`)
- **Windows**: `%LOCALAPPDATA%\thocci\`
- **macOS**: `~/Library/Application Support/thocci/`

Daemon logs are written to `thocci.log` inside the state directory.

## Building from source

### CLI only (no WebKit needed)

```bash
# Linux requires ALSA headers (e.g. libasound2-dev / alsa-lib)
cargo build --release
./target/release/thocci on
```

### Tauri desktop shell

```bash
cargo build -p thocci-app
# or run directly:
cargo run -p thocci-app
```

## Privacy

Thocci only reads physical keycodes to trigger audio playback. It never logs, stores, or transmits what you type.
