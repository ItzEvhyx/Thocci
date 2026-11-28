//! Background daemon: listen for global key presses, mix audio, and serve
//! control commands over localhost TCP.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::time::{Duration, Instant};

use crate::audio::{apply_low_latency_env, AudioEngine, ChannelGains};
use crate::control::ControlServer;
use crate::input::{create_listener, KeyEvent, KeyEventKind};
use crate::keymap;
use crate::paths::{
    clamp_percent, ensure_state_dir, owner_file, parse_stereo_flag, pid_file, read_pid_file,
    read_saved_pack, read_saved_stereo, read_saved_volume, remove_if_exists, write_daemon_owner,
    write_saved_pack, write_saved_stereo, write_saved_volume, SessionOwner,
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
    /// When set, the daemon is owned by this desktop app PID and exits
    /// automatically if the owning process terminates (owner loss).
    pub owner_pid: Option<u32>,
}

struct SharedState {
    audio: AudioEngine,
    pack: RwLock<Arc<Soundpack>>,
    pack_id: Mutex<String>,
    volume_percent: AtomicU8,
    stereo: AtomicBool,
    owner: SessionOwner,
    ready: AtomicBool,
    stop: AtomicBool,
    audio_recovery_pending: AtomicBool,
    stop_lock: Mutex<()>,
    stop_signal: Condvar,
}

impl SharedState {
    fn request_stop(&self) {
        self.stop.store(true, Ordering::Release);
        self.ready.store(false, Ordering::Release);
        self.audio.clear();
        let _guard = self.stop_lock.lock().unwrap_or_else(|e| e.into_inner());
        self.stop_signal.notify_all();
    }

    fn request_audio_recovery(&self) {
        self.audio_recovery_pending.store(true, Ordering::Release);
        let _guard = self.stop_lock.lock().unwrap_or_else(|e| e.into_inner());
        self.stop_signal.notify_all();
    }

    fn sync_pack_sample_rate(&self) {
        let target_rate = self.audio.sample_rate();
        let current_pack = {
            let guard = self.pack.read().unwrap_or_else(|e| e.into_inner());
            Arc::clone(&guard)
        };
        if current_pack.sample_rate() == Some(target_rate) {
            return;
        }
        let resampled = Arc::new(current_pack.resampled_to(target_rate));
        let mut guard = self.pack.write().unwrap_or_else(|e| e.into_inner());
        // Only swap if the pack wasn't switched concurrently to a different pack.
        if guard.info().id == resampled.info().id && guard.sample_rate() != Some(target_rate) {
            *guard = resampled;
            log::info!(
                "Resampled active soundpack '{}' to {} Hz for new output device",
                guard.info().name,
                target_rate,
            );
        }
    }

    fn handle_command(&self, raw: &str) -> String {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return "ERR empty".to_owned();
        }
        let mut parts = trimmed.split_whitespace();
        let cmd = parts.next().unwrap_or("").to_ascii_lowercase();

        match cmd.as_str() {
            "ping" => {
                if self.stop.load(Ordering::Acquire) {
                    "ERR stopping".to_owned()
                } else if !self.ready.load(Ordering::Acquire) {
                    "ERR starting".to_owned()
                } else {
                    "OK pong".to_owned()
                }
            }
            "status" => {
                if self.stop.load(Ordering::Acquire) {
                    return "ERR stopping".to_owned();
                }
                let vol = self.volume_percent.load(Ordering::Relaxed);
                let stereo = if self.stereo.load(Ordering::Acquire) {
                    "on"
                } else {
                    "off"
                };
                let pack = self
                    .pack_id
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone();
                let owner_tag = self.owner.to_tag();
                format!("OK running volume={vol} pack={pack} stereo={stereo} owner={owner_tag}")
            }
            "volume" => self.cmd_volume(parts.next()),
            "pack" => self.cmd_pack(parts.next()),
            "stereo" => self.cmd_stereo(parts.next()),
            "stop" => {
                self.request_stop();
                "OK stopping".to_owned()
            }
            other => format!("ERR unknown:{other}"),
        }
    }

    fn cmd_stereo(&self, arg: Option<&str>) -> String {
        let Some(enabled) = arg.and_then(parse_stereo_flag) else {
            return "ERR stereo must be on or off".to_owned();
        };
        self.stereo.store(enabled, Ordering::Release);
        let _ = write_saved_stereo(enabled);
        let label = if enabled { "on" } else { "off" };
        log::info!("Stereo keystroke panning set to {label}");
        format!("OK stereo={label}")
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
        let new_pack = match Soundpack::load_by_id(pack_id, target_rate) {
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

    fn on_key_event(&self, event: KeyEvent) {
        if !self.ready.load(Ordering::Acquire) || self.stop.load(Ordering::Acquire) {
            return;
        }
        let pack = {
            let guard = self.pack.read().unwrap_or_else(|e| e.into_inner());
            Arc::clone(&guard)
        };
        let Some(sound) = pack
            .sound_for_linux_key(event.code)
            .or_else(|| pack.representative_sound())
        else {
            return;
        };
        let stereo_enabled = self.stereo.load(Ordering::Acquire);
        let gains = ChannelGains::for_zone(keymap::key_zone(event.code), stereo_enabled);
        match event.kind {
            KeyEventKind::Down => self.audio.play_clip_with_gains(&sound.down, gains),
            KeyEventKind::Up => {
                if let Some(release) = &sound.up {
                    self.audio.play_clip_with_gains(release, gains);
                }
            }
        }
    }
}

/// Run the background daemon in the current process until stopped.
pub fn run_daemon(config: DaemonConfig) -> Result<()> {
    apply_low_latency_env();
    init_logging();
    ensure_state_dir()?;
    let owner = config
        .owner_pid
        .filter(|&pid| pid > 0)
        .map(SessionOwner::Desktop)
        .unwrap_or(SessionOwner::Cli);
    claim_pid_file(owner)?;

    let result = run_daemon_inner(config, owner);
    release_pid_file();
    log::info!("Thocci daemon stopped");
    result
}

fn run_daemon_inner(config: DaemonConfig, owner: SessionOwner) -> Result<()> {
    if let SessionOwner::Desktop(owner_pid) = owner {
        if !pid_is_running(owner_pid) {
            log::info!("Desktop owner pid {owner_pid} exited before startup completed; aborting");
            return Ok(());
        }
    }

    let volume_percent = config.volume.unwrap_or_else(read_saved_volume).min(100);
    // Decode the initial soundpack BEFORE opening the live audio stream so OGG/WAV
    // decoding never starves a freshly opened Bluetooth/headphone stream with xruns.
    let preferred_rate = AudioEngine::preferred_sample_rate();
    let (mut pack, pack_id) = load_initial_pack(&config, preferred_rate)?;

    let audio = AudioEngine::new(f32::from(volume_percent) / 100.0);
    audio.init()?;
    let active_rate = audio.sample_rate();
    if pack.sample_rate() != Some(active_rate) {
        pack = pack.resampled_to(active_rate);
    }
    audio.reset_watchdog();
    log::info!(
        "Loaded soundpack '{}' ({}, {} keys, {} Hz) at volume {}% (owner={})",
        pack.info().name,
        pack_id,
        pack.key_count(),
        active_rate,
        volume_percent,
        owner.to_tag(),
    );

    let stereo_enabled = read_saved_stereo();
    let state = Arc::new(SharedState {
        audio,
        pack: RwLock::new(Arc::new(pack)),
        pack_id: Mutex::new(pack_id),
        volume_percent: AtomicU8::new(volume_percent),
        stereo: AtomicBool::new(stereo_enabled),
        owner,
        ready: AtomicBool::new(false),
        stop: AtomicBool::new(false),
        audio_recovery_pending: AtomicBool::new(false),
        stop_lock: Mutex::new(()),
        stop_signal: Condvar::new(),
    });

    let wake_state = Arc::clone(&state);
    state
        .audio
        .set_recovery_notifier(Arc::new(move || wake_state.request_audio_recovery()));

    install_signal_handler(Arc::clone(&state));

    let mut listener = create_listener()?;
    let key_state = Arc::clone(&state);
    listener.start(Arc::new(move |event| key_state.on_key_event(event)))?;

    let ctrl_state = Arc::clone(&state);
    let mut control = ControlServer::start(Arc::new(move |cmd| ctrl_state.handle_command(cmd)))?;

    // Only enable keystroke playback once both the input listener and control
    // server are active and stoppable.
    if !state.stop.load(Ordering::Acquire) {
        state.ready.store(true, Ordering::Release);
    }

    let device_check_interval = Duration::from_secs(2);
    let owner_check_interval = Duration::from_millis(250);
    let mut last_device_check = Instant::now();
    {
        let mut stop_guard = state.stop_lock.lock().unwrap_or_else(|e| e.into_inner());
        while !state.stop.load(Ordering::Acquire) {
            if let SessionOwner::Desktop(owner_pid) = state.owner {
                if !pid_is_running(owner_pid) {
                    log::info!(
                        "Desktop owner pid {owner_pid} is no longer running; stopping desktop playback session"
                    );
                    drop(stop_guard);
                    state.request_stop();
                    break;
                }
            }

            let recovery_requested = state.audio_recovery_pending.swap(false, Ordering::AcqRel);
            let remaining_device =
                device_check_interval.saturating_sub(last_device_check.elapsed());
            if recovery_requested || remaining_device.is_zero() {
                drop(stop_guard);
                if state.audio.check_device_change() {
                    state.sync_pack_sample_rate();
                }
                last_device_check = Instant::now();
                stop_guard = state.stop_lock.lock().unwrap_or_else(|e| e.into_inner());
                continue;
            }
            let wait_for = match state.owner {
                SessionOwner::Desktop(_) => remaining_device.min(owner_check_interval),
                SessionOwner::Cli => remaining_device,
            };
            let (next_guard, _) = state
                .stop_signal
                .wait_timeout(stop_guard, wait_for)
                .unwrap_or_else(|e| e.into_inner());
            stop_guard = next_guard;
        }
    }
    state.ready.store(false, Ordering::Release);

    // Silence & drop audio immediately before joining the listener/control threads
    // so Disable/Quit/owner-loss never emits trailing thocks or held-key releases.
    state.audio.shutdown();
    listener.stop();
    control.stop();
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
    let pack = Soundpack::load_by_id(&pack_id, target_rate)?;
    Ok((pack, pack_id))
}

fn install_signal_handler(state: Arc<SharedState>) {
    let _ = ctrlc::set_handler(move || {
        log::info!("Received termination signal, shutting down");
        state.request_stop();
    });
}

fn init_logging() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_secs()
        .try_init();
}

fn claim_pid_file(owner: SessionOwner) -> Result<()> {
    if let Some(old_pid) = read_pid_file() {
        if old_pid != std::process::id() && pid_is_running(old_pid) {
            return Err(Error::new(format!(
                "Thocci is already running (pid {old_pid})"
            )));
        }
        remove_if_exists(pid_file());
        remove_if_exists(owner_file());
    }
    std::fs::write(pid_file(), std::process::id().to_string())?;
    let _ = write_daemon_owner(owner);
    Ok(())
}

fn release_pid_file() {
    if read_pid_file() == Some(std::process::id()) {
        remove_if_exists(pid_file());
        remove_if_exists(owner_file());
    }
}
