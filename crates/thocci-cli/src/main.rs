//! `thocci` — Mechvibes-style mechanical keyboard sounds CLI & daemon.
//!
//! Commands mirror Mechy 1:1:
//!   thocci on | off | toggle | status | volume <0-100> | cherry-mx | banana-split
//! Plus:
//!   thocci packs       list built-in soundpacks
//!   thocci uninstall   stop daemon and remove local installation

use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use thocci_core::client::{self, StartOptions};
use thocci_core::daemon::{self, DaemonConfig};
use thocci_core::paths;
use thocci_core::soundpack::{self, DEFAULT_PACK_ID};

#[derive(Parser)]
#[command(
    name = "thocci",
    version = thocci_core::VERSION,
    about = "Mechvibes-style mechanical keyboard sounds for Linux, Windows, and macOS"
)]
struct Cli {
    /// Print the post-install welcome banner (used by install.sh / install.ps1).
    #[arg(long, hide = true)]
    welcome: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Start keystroke sounds in the background
    On {
        /// Path to a custom Mechvibes soundpack directory
        #[arg(long)]
        pack: Option<PathBuf>,

        /// Initial volume 0-100 (default: last saved or 100)
        #[arg(long, value_name = "N")]
        volume: Option<i64>,
    },

    /// Stop keystroke sounds
    Off,

    /// Toggle keystroke sounds on/off
    Toggle {
        /// Path to a custom Mechvibes soundpack directory
        #[arg(long)]
        pack: Option<PathBuf>,

        /// Initial volume 0-100
        #[arg(long, value_name = "N")]
        volume: Option<i64>,
    },

    /// Show whether Thocci is running
    Status,

    /// Set volume (0-100)
    Volume {
        /// Volume percent 0-100
        level: i64,
    },

    /// Switch to the Cherry MX soundpack
    #[command(name = "cherry-mx")]
    CherryMx,

    /// Switch to the Banana Split soundpack
    #[command(name = "banana-split")]
    BananaSplit,

    /// Switch to a named built-in soundpack
    Pack {
        /// Built-in pack id (e.g. cherry-mx, banana-split)
        id: String,
    },

    /// List built-in soundpacks
    Packs,

    /// Remove Thocci state files and the installed binary
    Uninstall {
        /// Skip confirmation prompt
        #[arg(short = 'y', long)]
        yes: bool,
    },

    /// Internal: run the background daemon in the foreground
    #[command(hide = true)]
    Daemon {
        #[arg(long)]
        pack: Option<PathBuf>,

        #[arg(long = "pack-id")]
        pack_id: Option<String>,

        #[arg(long)]
        volume: Option<i64>,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    if cli.welcome {
        print_welcome();
        return ExitCode::SUCCESS;
    }

    let Some(command) = cli.command else {
        // Re-run clap help when invoked with no subcommand.
        let _ = Cli::parse_from(["thocci", "--help"]);
        return ExitCode::SUCCESS;
    };

    let code = match command {
        Command::On { pack, volume } => cmd_on(pack, volume),
        Command::Off => cmd_off(),
        Command::Toggle { pack, volume } => cmd_toggle(pack, volume),
        Command::Status => cmd_status(),
        Command::Volume { level } => cmd_volume(level),
        Command::CherryMx => cmd_switch_pack("cherry-mx"),
        Command::BananaSplit => cmd_switch_pack("banana-split"),
        Command::Pack { id } => cmd_switch_pack(&id),
        Command::Packs => cmd_packs(),
        Command::Uninstall { yes } => cmd_uninstall(yes),
        Command::Daemon {
            pack,
            pack_id,
            volume,
        } => cmd_daemon(pack, pack_id, volume),
    };

    ExitCode::from(code)
}

fn cmd_on(pack: Option<PathBuf>, volume: Option<i64>) -> u8 {
    if let Some(pid) = client::running_pid() {
        println!("already running (pid {pid})");
        return 0;
    }

    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(err) => {
            eprintln!("error: could not locate thocci executable: {err}");
            return 1;
        }
    };

    let label = pack
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(paths::read_saved_pack);

    match client::start(
        &exe,
        StartOptions {
            pack_dir: pack,
            volume,
        },
    ) {
        Ok(status) => {
            let pid = status.pid.unwrap_or(0);
            println!("on (pid {pid}, pack {label})");
            0
        }
        Err(err) => {
            eprintln!("error: {err}");
            1
        }
    }
}

fn cmd_off() -> u8 {
    match client::stop() {
        Ok(true) => {
            println!("off");
            0
        }
        Ok(false) => {
            println!("already stopped");
            0
        }
        Err(err) => {
            eprintln!("error: {err}");
            1
        }
    }
}

fn cmd_toggle(pack: Option<PathBuf>, volume: Option<i64>) -> u8 {
    if client::running_pid().is_none() {
        cmd_on(pack, volume)
    } else {
        cmd_off()
    }
}

fn cmd_status() -> u8 {
    let st = client::status();
    if !st.running {
        println!("stopped (pack {})", st.pack);
        return 1;
    }
    let pid = st.pid.unwrap_or(0);
    println!("running (pid {pid}, volume={} pack={})", st.volume, st.pack);
    0
}

fn cmd_volume(level: i64) -> u8 {
    match client::set_volume(level) {
        Ok((percent, true)) => {
            println!("volume {percent}");
            0
        }
        Ok((percent, false)) => {
            println!("volume {percent} (saved; daemon stopped)");
            0
        }
        Err(err) => {
            eprintln!("{err}");
            1
        }
    }
}

fn cmd_switch_pack(pack_id: &str) -> u8 {
    match client::set_pack(pack_id) {
        Ok(true) => {
            println!("pack {pack_id}");
            0
        }
        Ok(false) => {
            println!("pack {pack_id} (saved; will use on next thocci on)");
            0
        }
        Err(err) => {
            eprintln!("error: {err}");
            1
        }
    }
}

fn cmd_packs() -> u8 {
    let active = paths::read_saved_pack();
    let width = soundpack::BUILTIN_PACKS
        .iter()
        .map(|p| p.id.len())
        .max()
        .unwrap_or(14)
        + 2;
    for pack in soundpack::BUILTIN_PACKS {
        let marker = if pack.id == active { "*" } else { " " };
        let default_tag = if pack.id == DEFAULT_PACK_ID {
            " (default)"
        } else {
            ""
        };
        println!(
            "{marker} {:<width$} {}{default_tag}",
            pack.id, pack.display_name
        );
    }
    0
}

fn cmd_daemon(pack: Option<PathBuf>, pack_id: Option<String>, volume: Option<i64>) -> u8 {
    let config = DaemonConfig {
        pack_dir: pack,
        pack_id,
        volume: volume.map(paths::clamp_percent),
    };
    match daemon::run_daemon(config) {
        Ok(()) => 0,
        Err(err) => {
            eprintln!("error: {err}");
            1
        }
    }
}

fn cmd_uninstall(yes: bool) -> u8 {
    if !yes {
        print!("Uninstall Thocci and remove its state directory? [y/N]: ");
        let _ = io::stdout().flush();
        let mut answer = String::new();
        if io::stdin().read_line(&mut answer).is_err() {
            eprintln!("Uninstall aborted.");
            return 1;
        }
        let trimmed = answer.trim();
        if !trimmed.eq_ignore_ascii_case("y") && !trimmed.eq_ignore_ascii_case("yes") {
            println!("Uninstall aborted.");
            return 0;
        }
    }

    let _ = client::stop();

    let state = paths::state_dir();
    if state.exists() {
        if let Err(err) = std::fs::remove_dir_all(&state) {
            eprintln!("warning: could not remove {}: {err}", state.display());
        } else {
            println!("Removed state directory: {}", state.display());
        }
    }

    if let Ok(exe) = std::env::current_exe() {
        #[cfg(unix)]
        {
            if let Err(err) = std::fs::remove_file(&exe) {
                eprintln!("warning: could not remove {}: {err}", exe.display());
            } else {
                println!("Removed binary: {}", exe.display());
            }
        }
        #[cfg(windows)]
        {
            // On Windows a running binary cannot delete itself directly; schedule
            // deletion right after this process exits.
            let _ = std::process::Command::new("cmd")
                .args([
                    "/C",
                    "ping 127.0.0.1 -n 2 > nul & del /f /q",
                    &exe.to_string_lossy(),
                ])
                .spawn();
            println!("Scheduled binary removal: {}", exe.display());
        }
    }

    println!("Thocci uninstalled.");
    0
}

fn print_welcome() {
    println!(
        r#"
  _______ _                    _
 |__   __| |                  (_)
    | |  | |__   ___   ___ ___ _
    | |  | '_ \ / _ \ / __/ __| |
    | |  | | | | (_) | (_| (__| |
    |_|  |_| |_|\___/ \___\___|_|  v{version}

  Mechanical keyboard sounds for Linux, Windows, and macOS.

  QUICK START:
    thocci on                Start keystroke sounds
    thocci off               Stop keystroke sounds
    thocci toggle            Flip on/off
    thocci status            Check status, volume, and active pack
    thocci volume 50         Set volume (0-100)
    thocci cherry-mx         Switch to Cherry MX pack (default)
    thocci banana-split      Switch to Banana Split pack
    thocci packs             List built-in soundpacks
    thocci uninstall         Uninstall Thocci

  REPO:
    https://github.com/ItzEvhyx/Thocci
"#,
        version = thocci_core::VERSION
    );
}
