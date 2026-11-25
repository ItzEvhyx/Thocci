# ![Thocci — Ink perched on a black titanium Escape key](assets/readme/thocci-banner.webp)

<p align="center">
  <strong>Mechanical keyboard sounds for Windows, Linux, and macOS.</strong>
</p>

<p align="center">
  <a href="#installation">Install</a> ·
  <a href="#terminal-guide">Terminal guide</a> ·
  <a href="#privacy">Privacy</a> ·
  <a href="#meet-ink">Meet Ink</a>
</p>

Thocci gives your physical keyboard a new sound. Choose from 20 built-in soundpacks, from Cherry MX Black and soft creamy taps to a classic typewriter. Switch packs or adjust the volume while you type, or bring your own soundpack. Packs with separate press and release sounds can play both.

## Installation

The installers download the prebuilt CLI from [GitHub Releases](https://github.com/ItzEvhyx/Thocci/releases). Run them once a release with a download for your platform is available.

### Windows

Open PowerShell on Windows (x64) and run the installer.

```powershell
Invoke-RestMethod https://raw.githubusercontent.com/ItzEvhyx/Thocci/main/install.ps1 | Invoke-Expression
```

The installer places `thocci.exe` in `%USERPROFILE%\.local\bin` and adds that folder to your user `PATH`. Open a new PowerShell window if `thocci` is unavailable in the current one.

### Linux and macOS

Run these commands in your terminal. The installer supports x64 and ARM64 Linux systems with glibc, as well as Intel and Apple silicon Macs.

```bash
curl -fsSLo thocci-install.sh https://raw.githubusercontent.com/ItzEvhyx/Thocci/main/install.sh &&
  bash ./thocci-install.sh
```

The installer places `thocci` in `~/.local/bin`. If your shell cannot find `thocci`, use `~/.local/bin/thocci` or add that directory to your `PATH`.

- **Linux** requires access to keyboard input devices. Run `sudo usermod -aG input "$USER"`, then sign out and back in.
- **macOS** requires keyboard access. Under **System Settings → Privacy & Security → Input Monitoring**, allow your terminal or the Thocci binary, then reopen the terminal.

## Terminal guide

Start Thocci and type a few keys. To try a softer sound, switch to the Creamy pack and set the volume to 40%.

```bash
thocci on
thocci creamy
thocci volume 40
```

| Command | What it does |
| --- | --- |
| `thocci on` / `thocci off` | Start or stop keyboard sounds. |
| `thocci toggle` | Turn sounds on or off with one command. |
| `thocci packs` | Browse the 20 built-in soundpacks. |
| `thocci pack "Wooden Keyboard"` | Switch by pack ID, display name, or unique partial match. `thocci creamy` is the short form. |
| `thocci volume 40` | Set the volume from 0 to 100. |
| `thocci on --pack "./my-pack"` | Start with a custom soundpack folder. |
| `thocci guide` / `thocci --help` | Open the built-in terminal guide. |
| `thocci --plain guide` | Open the guide without ANSI colors. |

## Privacy

**Thocci has no data collection or telemetry.** While sounds are enabled, it listens for presses and releases on your physical keyboard solely to play the corresponding sounds. Playback happens on your device. Thocci never logs, stores, or sends what you type.

## Meet Ink

Ink is Thocci’s cat-eared mascot. His favorite spot is the spacebar.

![Ink perched, peeking, bouncing, pressing, stretching, and napping on black titanium keycaps](assets/readme/ink-character-sheet.webp)

Thocci is open source under the [MIT License](LICENSE).
