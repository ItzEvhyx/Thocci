# Thocci

Mechanical keyboard sounds for **Linux**, **Windows**, and **macOS**, written in Rust with a Tauri desktop shell.

Every physical keystroke plays a low-latency clip from an embedded or custom v1/v2 soundpack (including separate press + release audio for v2 packs). No Python, no `ffmpeg`, and no external asset folder required — all 20 built-in soundpacks are compiled directly into the standalone `thocci` binary.

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
- **Windows**: Runs out of the box via a low-level keyboard hook. Elevated apps (some game launchers/admin terminals) only deliver hooks if Thocci is also run elevated.
- **macOS**: Grant **Input Monitoring** (or **Accessibility**) permission to your terminal or Thocci in *System Settings → Privacy & Security*.

---

## Terminal Guide

Run `thocci`, `thocci --help`, or `thocci guide` in your terminal at any time to open the built-in, color-coded terminal guide (inspired by [ezCMD](https://github.com/ItzEvhyx/ezCMD)).

> [!NOTE]
> **Color-Coded Output:** On interactive terminals, commands are highlighted in **bold `#96d0fb`**, `<placeholders>` in **yellow**, active/running indicators in **green**, volume values in **amber**, and errors in **red**. Color automatically switches off in pipes, when `NO_COLOR` is set, or when `--plain` is passed.

### Engine Controls

| Command | What it does |
|---------|--------------|
| `thocci on` | Start keystroke sounds in the background |
| `thocci on --volume <0-100>` | Start at a specific initial volume |
| `thocci on --pack <dir>` | Start using a custom soundpack folder |
| `thocci off` | Stop the background daemon |
| `thocci toggle` | Flip sounds on/off (ideal to bind to a global hotkey) |
| `thocci status` | Show live running state, PID, volume, and active pack |

### Volume & Soundpack Switching

| Command | What it does |
|---------|--------------|
| `thocci volume <0-100>` | Change volume live (or save while stopped) |
| `thocci packs` | List all 20 built-in soundpacks with the active one highlighted |
| `thocci <pack>` | Switch soundpacks live by **slug**, **display name**, or **unique partial match** |
| `thocci pack <query>` | Explicit form of live pack switching |
| `thocci cherry-mx` | Quick alias for the Cherry MX pack (default) |
| `thocci banana-split` | Quick alias for the Banana Split pack |

### Everyday Examples

```bash
thocci on                     # Start typing with Cherry MX (default)
thocci volume 60              # Lower volume to 60% live
thocci packs                  # Browse all 20 built-in soundpacks
thocci creamy                 # Switch pack by slug
thocci Wooden Keyboard        # Switch pack by full display name
thocci matcha                 # Switch pack by unique partial match
thocci status                 # Check PID, volume, and active pack
thocci toggle                 # Flip on/off from a shortcut or terminal
thocci off                    # Stop the background daemon
```

### Flags & Maintenance

| Command / Flag | What it does |
|----------------|--------------|
| `thocci guide` / `thocci --help` | Open the colorized Terminal Guide |
| `thocci --plain <cmd>` | Disable ANSI colors (useful for scripts and logs) |
| `thocci --version` | Print installed Thocci version |
| `thocci uninstall` | Stop daemon and remove the Thocci binary and state files (`-y` skips prompt) |

---

## Built-in Soundpacks

All 20 soundpacks live in `assets/soundpacks/` and are embedded directly into `thocci` at build time (supporting both v1 and v2 press+release formats, `.ogg`, `.wav`, and `.mp3`):

| Pack ID | Soundpack Name | Quick Switch Example |
|---------|----------------|----------------------|
| `cherry-mx` | CherryMX Black - ABS keycaps *(default)* | `thocci cherry-mx` |
| `banana-split` | Banana Split Lubed by Akira | `thocci banana-split` |
| `aula-f75-leobog-reaper` | aula f75 max leobog reaper switch | `thocci aula` |
| `clicky-perfect` | clicky perfect | `thocci clicky` |
| `creamy` | creamy (still getting better) | `thocci creamy` |
| `fortnite-keyboard` | fortnite keyboard | `thocci fortnite` |
| `gravastar-v75` | GravaStar V75 (Ploszy ASMR rip) | `thocci gravastar` |
| `hayyoths-keyboard` | Hayyoth's keyboard | `thocci hayyoth` |
| `hyperx-alloy-cherry-mx-blue` | Hyperx Alloy Cherry MX Blue | `thocci hyperx` |
| `kailh-saker-mini` | CnT - Kailh Saker Mini Low Profile | `thocci kailh` |
| `keychron-dream-fn-v2` | Myilo Keychron Dream FN V2 | `thocci keychron` |
| `kiiboom-matcha-latte` | Presskey - KiiBOOM Matcha Latte | `thocci matcha` |
| `leobog-hi75-reaper` | LEOBOG Hi75 (Reaper Switches) | `thocci hi75` |
| `nuphy-halo75-v2-raspberry` | NuPhy Halo75 V2 (Raspberry Switches) | `thocci nuphy` |
| `orange-deluxe` | Orange deluxe - Press + Release separated | `thocci orange` |
| `satisfaction-bubblepop` | Satisfaction - Bubblepop | `thocci bubblepop` |
| `typewriter` | TypeWriter | `thocci typewriter` |
| `wallhack-k-001` | Wallhack K-001 | `thocci wallhack` |
| `womier-sk75-tmr` | womier sk75 tmr | `thocci womier` |
| `wooden-keyboard` | Wooden Keyboard | `thocci Wooden Keyboard` |

Switching soundpacks and adjusting volume happen live while Thocci is running.

---

## Architecture

Thocci is organized as a Rust workspace so the CLI stays lightweight and headless while the Tauri desktop app shares the exact same engine:

```text
Thocci/
├── assets/soundpacks/ # 20 built-in v1 & v2 soundpacks (auto-embedded)
├── crates/
│   ├── thocci-core/   # Soundpack decoder (Symphonia), low-latency mixer (cpal),
│   │                  # OS key listeners (evdev / Win32 hook / CGEventTap),
│   │                  # daemon, localhost TCP control server & client
│   └── thocci-cli/    # Standalone `thocci` CLI + daemon binary & terminal guide
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
