//! Background daemon: listen for global key presses, mix audio, and serve
//! control commands over localhost TCP.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::{Duration, Instant};

use crate::audio::{apply_low_latency_env, AudioEngine};
use crate::control::ControlServer;
use crate::input::create_listener;
use crate::paths::{
    clamp_percent, ensure_state_dir, pid_file, read_pid_file, read_saved_pack, read_saved_volume,
    remove_if_exists, write_saved_pack, write_saved_volume,
};
use crate::process::pid_is_running;
use crate::soundpack::Soundpack;
use crate::{Error, Result};

/// Startup parameters for [`run_daemon`].
#[derive(Debug, Clone, Default)]
pub struct DaemonConfig {
    pub pack_dir: Option<PathBuf>,
    pub pack_id: Option<String>,
    pub volume: Option<u8>,
}

struct SharedState {
    audio: AudioEngine,
    pack: RwLock<Arc<Soundpack>>,
    pack_id: Mutex<String>,
    volume_percent: AtomicU8,
    stop: AtomicBool,
}

impl SharedState {
    fn handle_command(&self, raw: &str) -> String {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return "ERR empty".to_owned();
        }
        let mut parts = trimmed.split_whitespace();
        let cmd = parts.next().unwrap_or("").to_ascii_lowercase();

        match cmd.as_str() {
            "ping" => "OK pong".to_owned(),
            "status" => {
                let vol = self.volume_percent.load(Ordering::Relaxed);
                let pack = self
                    .pack_id
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone();
                format!("OK running volume={vol} pack={pack}")
            }
            "volume" => self.cmd_volume(parts.next()),
            "pack" => self.cmd_pack(parts.next()),
            "stop" => {
                self.stop.store(true, Ordering::Release);
                "OK stopping".to_owned()
            }
            other => format!("ERR unknown:{other}"),
        }
    }

    fn cmd_volume(&self, arg: Option<&str>) -> String {
        let Some(raw_level) = arg else {
            return "ERR volume must be 0-100".to_owned();
        };
        let Ok(parsed) = raw_level.parse::<i64>() else {
            return "ERR volume must be 0-100".to_owned();
        };
        let percent = clamp_percent(parsed);
        self.volume_percent.store(percent, Ordering::Relaxed);
        self.audio.set_volume(f32::from(percent) / 100.0);
        let _ = write_saved_volume(i64::from(percent));
        format!("OK volume={percent}")
    }

    fn cmd_pack(&self, arg: Option<&str>) -> String {
        let Some(pack_id) = arg else {
            return "ERR missing pack id".to_owned();
        };
        let target_rate = Some(self.audio.sample_rate());
        let new_pack = match Soundpack::load_builtin(pack_id, target_rate) {
            Ok(pack) => pack,
            Err(err) => {
                log::error!("Failed to load pack {pack_id}: {err}");
                return format!("ERR load failed:{err}");
            }
        };

        let name = new_pack.info().name.clone();
        let keys = new_pack.key_count();
        {
            let mut pack_guard = self.pack.write().unwrap_or_else(|e| e.into_inner());
            *pack_guard = Arc::new(new_pack);
        }
        {
            let mut id_guard = self.pack_id.lock().unwrap_or_else(|e| e.into_inner());
            *id_guard = pack_id.to_owned();
        }
        let _ = write_saved_pack(pack_id);
        log::info!("Switched soundpack to '{name}' ({pack_id}, {keys} keys)");
        format!("OK pack={pack_id}")
    }

    fn on_key_down(&self, linux_code: u16) {
        let pack = {
            let guard = self.pack.read().unwrap_or_else(|e| e.into_inner());
            Arc::clone(&guard)
        };
        if let Some(clip) = pack.clip_for_linux_key(linux_code) {
            self.audio.play_clip(clip);
        }
    }
}

/// Run the background daemon in the current process until stopped.
pub fn run_daemon(config: DaemonConfig) -> Result<()> {
    apply_low_latency_env();
    init_logging();
    ensure_state_dir()?;
    claim_pid_file()?;

    let result = run_daemon_inner(config);
    release_pid_file();
    log::info!("Thocci daemon stopped");
    result
}

fn run_daemon_inner(config: DaemonConfig) -> Result<()> {
    let volume_percent = config.volume.unwrap_or_else(read_saved_volume).min(100);
    let audio = AudioEngine::new(f32::from(volume_percent) / 100.0);
    audio.init()?;

    let target_rate = Some(audio.sample_rate());
    let (pack, pack_id) = load_initial_pack(&config, target_rate)?;
    log::info!(
        "Loaded soundpack '{}' ({}, {} keys) at volume {}%",
        pack.info().name,
        pack_id,
        pack.key_count(),
        volume_percent,
    );

    let state = Arc::new(SharedState {
        audio,
        pack: RwLock::new(Arc::new(pack)),
        pack_id: Mutex::new(pack_id),
        volume_percent: AtomicU8::new(volume_percent),
        stop: AtomicBool::new(false),
    });

    install_signal_handler(Arc::clone(&state));

    let mut listener = create_listener()?;
    let key_state = Arc::clone(&state);
    listener.start(Arc::new(move |code| key_state.on_key_down(code)))?;

    let ctrl_state = Arc::clone(&state);
    let mut control = ControlServer::start(Arc::new(move |cmd| ctrl_state.handle_command(cmd)))?;

    let mut last_device_check = Instant::now();
    while !state.stop.load(Ordering::Acquire) {
        thread::sleep(Duration::from_millis(100));
        if last_device_check.elapsed() >= Duration::from_secs(2) {
            state.audio.check_device_change();
            last_device_check = Instant::now();
        }
    }

    listener.stop();
    control.stop();
    state.audio.shutdown();
    Ok(())
}

fn load_initial_pack(
    config: &DaemonConfig,
    target_rate: Option<u32>,
) -> Result<(Soundpack, String)> {
    if let Some(dir) = &config.pack_dir {
        let pack = Soundpack::load_from_dir(dir, config.pack_id.as_deref(), target_rate)?;
        let id = pack.info().id.clone();
        return Ok((pack, id));
    }
    let pack_id = config.pack_id.clone().unwrap_or_else(read_saved_pack);
    let pack = Soundpack::load_builtin(&pack_id, target_rate)?;
    Ok((pack, pack_id))
}

fn install_signal_handler(state: Arc<SharedState>) {
    let _ = ctrlc::set_handler(move || {
        log::info!("Received termination signal, shutting down");
        state.stop.store(true, Ordering::Release);
    });
}

fn init_logging() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_secs()
        .try_init();
}

fn claim_pid_file() -> Result<()> {
    if let Some(old_pid) = read_pid_file() {
        if old_pid != std::process::id() && pid_is_running(old_pid) {
            return Err(Error::new(format!(
                "Thocci is already running (pid {old_pid})"
            )));
        }
        remove_if_exists(pid_file());
    }
    std::fs::write(pid_file(), std::process::id().to_string())?;
    Ok(())
}

fn release_pid_file() {
    if read_pid_file() == Some(std::process::id()) {
        remove_if_exists(pid_file());
    }
}
