//! `thocci` — Mechanical keyboard sounds CLI & daemon.
//!
//! Run `thocci`, `thocci --help`, or `thocci guide` for the colored terminal guide.

mod guide;
mod term;

use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use thocci_core::client::{self, StartOptions};
use thocci_core::daemon::{self, DaemonConfig};
use thocci_core::paths;
use thocci_core::soundpack;

#[derive(Parser)]
#[command(
    name = "thocci",
    version = thocci_core::VERSION,
    about = "Mechanical keyboard sounds for Linux, Windows, and macOS",
    disable_help_flag = true,
    disable_help_subcommand = true
)]
struct Cli {
    /// Show the comprehensive colored terminal guide
    #[arg(short = 'h', long = "help", global = true)]
    help: bool,

    /// Disable ANSI colors (also respected automatically via NO_COLOR or pipes)
    #[arg(long, global = true)]
    plain: bool,

    /// Print the post-install welcome banner (used by install.sh / install.ps1)
    #[arg(long, hide = true)]
    welcome: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Start keystroke sounds in the background
    On {
        /// Path to a custom soundpack directory
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
        /// Path to a custom soundpack directory
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

    /// Switch to a built-in soundpack by id or name (or run `thocci <pack>`)
    Pack {
        /// Soundpack id or name (e.g. creamy, wooden-keyboard, "Wooden Keyboard")
        #[arg(required = true)]
        query: Vec<String>,
    },

    /// List built-in soundpacks
    Packs,

    /// Show the comprehensive terminal guide
    #[command(alias = "help")]
    Guide,

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

    /// Direct soundpack switch shorthand (e.g. `thocci creamy`, `thocci Wooden Keyboard`)
    #[command(external_subcommand)]
    DirectPack(Vec<String>),
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let color = term::color_enabled(cli.plain);

    if cli.welcome {
        let mut out = io::stdout().lock();
        let _ = guide::print_welcome(&mut out, color);
        return ExitCode::SUCCESS;
    }

    if cli.help || cli.command.is_none() {
        let mut out = io::stdout().lock();
        let _ = guide::print_guide(&mut out, color);
        return ExitCode::SUCCESS;
    }

    let code = match cli.command.unwrap() {
        Command::On { pack, volume } => cmd_on(pack, volume, color),
        Command::Off => cmd_off(color),
        Command::Toggle { pack, volume } => cmd_toggle(pack, volume, color),
        Command::Status => cmd_status(color),
        Command::Volume { level } => cmd_volume(level, color),
        Command::CherryMx => cmd_switch_pack("cherry-mx", color),
        Command::BananaSplit => cmd_switch_pack("banana-split", color),
        Command::Pack { query } => cmd_switch_pack(&query.join(" "), color),
        Command::DirectPack(args) => cmd_switch_pack(&args.join(" "), color),
        Command::Packs => cmd_packs(color),
        Command::Guide => {
            let mut out = io::stdout().lock();
            let _ = guide::print_guide(&mut out, color);
            0
        }
        Command::Uninstall { yes } => cmd_uninstall(yes, color),
        Command::Daemon {
            pack,
            pack_id,
            volume,
        } => cmd_daemon(pack, pack_id, volume),
    };

    ExitCode::from(code)
}

fn print_err(message: &str, color: bool) {
    if color {
        eprintln!("{}error:{} {message}", term::WARNING_RED, term::RESET);
    } else {
        eprintln!("error: {message}");
    }
}

fn cmd_on(pack: Option<PathBuf>, volume: Option<i64>, color: bool) -> u8 {
    if let Some(pid) = client::running_pid() {
        if client::is_responsive() {
            if color {
                println!(
                    "{}● already running{} {}(pid {pid}){}",
                    term::GREEN_BOLD,
                    term::RESET,
                    term::DIM,
                    term::RESET
                );
            } else {
                println!("already running (pid {pid})");
            }
            return 0;
        }
    }

    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(err) => {
            print_err(&format!("could not locate thocci executable: {err}"), color);
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
            if color {
                println!(
                    "{}● on{}  {}pid={}{}  {}pack={}{}",
                    term::GREEN_BOLD,
                    term::RESET,
                    term::DIM,
                    pid,
                    term::RESET,
                    term::CMD_COLOR,
                    label,
                    term::RESET
                );
            } else {
                println!("on (pid {pid}, pack {label})");
            }
            0
        }
        Err(err) => {
            print_err(&err.to_string(), color);
            1
        }
    }
}

fn cmd_off(color: bool) -> u8 {
    match client::stop() {
        Ok(true) => {
            if color {
                println!("{}○ off{}", term::DIM, term::RESET);
            } else {
                println!("off");
            }
            0
        }
        Ok(false) => {
            if color {
                println!("{}○ already stopped{}", term::DIM, term::RESET);
            } else {
                println!("already stopped");
            }
            0
        }
        Err(err) => {
            print_err(&err.to_string(), color);
            1
        }
    }
}

fn cmd_toggle(pack: Option<PathBuf>, volume: Option<i64>, color: bool) -> u8 {
    if client::running_pid().is_none() {
        cmd_on(pack, volume, color)
    } else {
        cmd_off(color)
    }
}

fn cmd_status(color: bool) -> u8 {
    let st = client::status();
    let mut out = io::stdout().lock();
    let _ = guide::print_status(&mut out, &st, color);
    if st.running {
        0
    } else {
        1
    }
}

fn cmd_volume(level: i64, color: bool) -> u8 {
    match client::set_volume(level) {
        Ok((percent, true)) => {
            if color {
                println!(
                    "{}✓ volume{} {}{percent}%{}",
                    term::GREEN_BOLD,
                    term::RESET,
                    term::AMBER_BOLD,
                    term::RESET
                );
            } else {
                println!("volume {percent}");
            }
            0
        }
        Ok((percent, false)) => {
            if color {
                println!(
                    "{}✓ volume{} {}{percent}%{} {}(saved; daemon stopped){}",
                    term::GREEN_BOLD,
                    term::RESET,
                    term::AMBER_BOLD,
                    term::RESET,
                    term::DIM,
                    term::RESET
                );
            } else {
                println!("volume {percent} (saved; daemon stopped)");
            }
            0
        }
        Err(err) => {
            print_err(&err.to_string(), color);
            1
        }
    }
}

fn cmd_switch_pack(query: &str, color: bool) -> u8 {
    let pack = match soundpack::resolve_any_pack_query(query) {
        Ok(pack) => pack,
        Err(err) => {
            print_err(&err.to_string(), color);
            return 1;
        }
    };
    match client::set_pack(&pack.id) {
        Ok(true) => {
            if color {
                println!(
                    "{}✓ pack{} {}{}{} {}({}){}",
                    term::GREEN_BOLD,
                    term::RESET,
                    term::CMD_COLOR,
                    pack.id,
                    term::RESET,
                    term::DIM,
                    pack.raw_name,
                    term::RESET
                );
            } else {
                println!("pack {} ({})", pack.id, pack.raw_name);
            }
            0
        }
        Ok(false) => {
            if color {
                println!(
                    "{}✓ pack{} {}{}{} {}({}; saved, will use on next thocci on){}",
                    term::GREEN_BOLD,
                    term::RESET,
                    term::CMD_COLOR,
                    pack.id,
                    term::RESET,
                    term::DIM,
                    pack.raw_name,
                    term::RESET
                );
            } else {
                println!(
                    "pack {} ({}; saved, will use on next thocci on)",
                    pack.id, pack.raw_name
                );
            }
            0
        }
        Err(err) => {
            print_err(&err.to_string(), color);
            1
        }
    }
}

fn cmd_packs(color: bool) -> u8 {
    let active = paths::read_saved_pack();
    let mut out = io::stdout().lock();
    let _ = guide::print_packs(&mut out, &active, color);
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

fn cmd_uninstall(yes: bool, color: bool) -> u8 {
    if !yes {
        if color {
            print!(
                "{}Uninstall Thocci and remove its state directory? [y/N]:{} ",
                term::AMBER_BOLD,
                term::RESET
            );
        } else {
            print!("Uninstall Thocci and remove its state directory? [y/N]: ");
        }
        let _ = io::stdout().flush();
        let mut answer = String::new();
        if io::stdin().read_line(&mut answer).is_err() {
            print_err("Uninstall aborted.", color);
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

    if color {
        println!("{}✓ Thocci uninstalled.{}", term::GREEN_BOLD, term::RESET);
    } else {
        println!("Thocci uninstalled.");
    }
    0
}
