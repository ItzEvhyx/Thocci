#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::process::ExitCode;

use thocci_core::daemon::{self, DaemonConfig};
use thocci_core::paths;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();

    // Allow `thocci-app daemon ...` as a standalone fallback when the `thocci`
    // CLI binary is not placed next to the desktop app.
    if args.get(1).map(String::as_str) == Some("daemon") {
        let config = parse_daemon_args(&args[2..]);
        return match daemon::run_daemon(config) {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("error: {err}");
                ExitCode::FAILURE
            }
        };
    }

    thocci_app_lib::run();
    ExitCode::SUCCESS
}

fn parse_daemon_args(args: &[String]) -> DaemonConfig {
    let mut config = DaemonConfig::default();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--pack" if i + 1 < args.len() => {
                config.pack_dir = Some(PathBuf::from(&args[i + 1]));
                i += 2;
            }
            "--pack-id" if i + 1 < args.len() => {
                config.pack_id = Some(args[i + 1].clone());
                i += 2;
            }
            "--volume" if i + 1 < args.len() => {
                if let Ok(val) = args[i + 1].parse::<i64>() {
                    config.volume = Some(paths::clamp_percent(val));
                }
                i += 2;
            }
            "--owner-pid" if i + 1 < args.len() => {
                if let Ok(pid) = args[i + 1].parse::<u32>() {
                    if pid > 0 {
                        config.owner_pid = Some(pid);
                    }
                }
                i += 2;
            }
            _ => i += 1,
        }
    }
    config
}
