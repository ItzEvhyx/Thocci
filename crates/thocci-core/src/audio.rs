//! Low-latency overlapping keystroke mixer backed by `cpal`.
//!
//! - Uses PipeWire/PulseAudio low-latency environment hints plus explicit
//!   bounded buffer ladders on Linux, WASAPI on Windows, and CoreAudio on macOS
//!   so wired headsets, USB DACs, and Bluetooth headphones/earbuds never
//!   negotiate multi-second ring buffers.
//! - Mixes up to `MAX_VOICES` overlapping stereo clips inside the audio callback
//!   with zero heap allocation on the real-time thread.
//! - Automatically and promptly reopens the output stream when the default
//!   output device changes, reroutes, disconnects, stalls, or hits repeated
//!   underruns / runaway ring-buffer latency.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{
    BufferSize, Device, DeviceId, ErrorKind, FromSample, SampleFormat, SizedSample, Stream,
    StreamConfig, SupportedBufferSize, SupportedStreamConfig,
};

use crate::keymap::KeyZone;
use crate::soundpack::{resample_stereo, SoundClip};
use crate::{Error, Result};

/// Attenuation applied to the opposite channel at the ±70% stereo side zones,
/// matching equal-power panning's left-to-right ratio (~0.24) without boosting
/// either channel of a stereo source clip.
const STEREO_SIDE_ATTENUATION: f32 = 0.24;

/// Per-voice left/right gain multipliers computed off the audio thread.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChannelGains {
    pub left: f32,
    pub right: f32,
}

impl ChannelGains {
    pub const UNCHANGED: Self = Self {
        left: 1.0,
        right: 1.0,
    };

    /// Compute attenuation-only stereo channel gains for a physical key zone.
    ///
    /// Returns `(1.0, 1.0)` when stereo panning is disabled or for centered keys.
    pub fn for_zone(zone: KeyZone, stereo_enabled: bool) -> Self {
        if !stereo_enabled {
            return Self::UNCHANGED;
        }
        match zone {
            KeyZone::Left => Self {
                left: 1.0,
                right: STEREO_SIDE_ATTENUATION,
            },
            KeyZone::Center => Self::UNCHANGED,
            KeyZone::Right => Self {
                left: STEREO_SIDE_ATTENUATION,
                right: 1.0,
            },
        }
    }
}

const MAX_VOICES: usize = 8;
/// Discard unstarted voices that waited longer than this (e.g. during a device
/// handoff, Bluetooth glitch, or xrun) so delayed keystrokes never fire
/// phantom clicks after the user stops typing.
const MAX_VOICE_STALENESS: Duration = Duration::from_millis(75);
/// Reopen the output stream if the audio callback stops advancing for this long.
const CALLBACK_STALL_TIMEOUT: Duration = Duration::from_millis(1500);
/// Poll timeout passed to `cpal` so ALSA's xrun/suspend recovery path can fire
/// even if `POLLERR`/`POLLHUP` is missed by the driver or PipeWire bridge.
const STREAM_POLL_TIMEOUT: Duration = Duration::from_millis(150);
/// Consecutive xruns observed before forcing a clean stream reopen.
const XRUN_REOPEN_STREAK: u32 = 4;
/// If a previously healthy stream's reported output ring-buffer delay balloons
/// past this threshold for `HIGH_LATENCY_REOPEN_STREAK` consecutive callbacks,
/// trigger a clean one-shot reopen to clear runaway driver/Bluetooth buffering.
const HIGH_LATENCY_REOPEN_THRESHOLD_MS: u32 = 500;
const HIGH_LATENCY_RESET_THRESHOLD_MS: u32 = 300;
const HIGH_LATENCY_REOPEN_STREAK: u32 = 16;

type RecoveryNotifier = Arc<dyn Fn() + Send + Sync>;

/// Hint PipeWire / PulseAudio toward a stable low-latency quantum (Linux only).
///
/// Must be called early during process startup before opening audio streams
/// (or initializing GTK/WebKit in the desktop app).
pub fn apply_low_latency_env() {
    #[cfg(target_os = "linux")]
    {
        if std::env::var_os("PIPEWIRE_LATENCY").is_none() {
            std::env::set_var("PIPEWIRE_LATENCY", "512/48000");
        }
        if std::env::var_os("PULSE_LATENCY_MSEC").is_none() {
            std::env::set_var("PULSE_LATENCY_MSEC", "15");
        }
        if std::env::var_os("PULSE_PROP_media.role").is_none() {
            std::env::set_var("PULSE_PROP_media.role", "event");
        }
        let existing = std::env::var("PIPEWIRE_PROPS").unwrap_or_default();
        if !existing.contains("media.role=") {
            let props = format!("{existing} media.role=Event");
            std::env::set_var("PIPEWIRE_PROPS", props.trim());
        }
    }
}

struct Voice {
    /// Interleaved stereo `f32` samples matching the stream's sample rate.
    samples: Arc<[f32]>,
    /// Current index into `samples` (always even, advances by 2 per frame).
    cursor: usize,
    /// Per-voice channel gains applied when rendering to stereo/multichannel output.
    left_gain: f32,
    right_gain: f32,
    /// When the keystroke queued this voice; stale unstarted voices are dropped.
    queued_at: Instant,
}

struct MixerShared {
    voices: Mutex<Vec<Voice>>,
    /// Lock-free fast-path flag so silent real-time callbacks avoid mutex & clock calls.
    has_voices: AtomicBool,
    /// Volume as `f32::to_bits()`, clamped to `[0.0, 1.0]`.
    volume_bits: AtomicU32,
    needs_reopen: AtomicBool,
    /// Monotonically incremented by every `render()` invocation to detect stalls.
    callback_ticks: AtomicU64,
    /// Consecutive xruns observed before forcing a clean stream reopen.
    xrun_streak: AtomicU32,
    /// Latest stream playback-minus-callback delay in milliseconds (for telemetry & drift detection).
    last_output_delay_ms: AtomicU32,
    /// True once the current stream has observed normal latency (`<= 300 ms`).
    saw_healthy_latency: AtomicBool,
    /// Consecutive callbacks with runaway ring-buffer latency (`>= 500 ms`).
    high_latency_streak: AtomicU32,
    /// Ensures runaway-latency recovery does not loop on hardware with fixed high delay.
    high_latency_reopen_attempted: AtomicBool,
    /// Optional wakeup hook so the daemon reopens promptly instead of waiting for a poll tick.
    recovery_notifier: Mutex<Option<RecoveryNotifier>>,
}

impl MixerShared {
    fn new(volume: f32) -> Self {
        Self {
            voices: Mutex::new(Vec::with_capacity(MAX_VOICES)),
            has_voices: AtomicBool::new(false),
            volume_bits: AtomicU32::new(volume.clamp(0.0, 1.0).to_bits()),
            needs_reopen: AtomicBool::new(false),
            callback_ticks: AtomicU64::new(0),
            xrun_streak: AtomicU32::new(0),
            last_output_delay_ms: AtomicU32::new(0),
            saw_healthy_latency: AtomicBool::new(false),
            high_latency_streak: AtomicU32::new(0),
            high_latency_reopen_attempted: AtomicBool::new(false),
            recovery_notifier: Mutex::new(None),
        }
    }

    fn reset_stream_health(&self) {
        self.needs_reopen.store(false, Ordering::Release);
        self.xrun_streak.store(0, Ordering::Relaxed);
        self.last_output_delay_ms.store(0, Ordering::Relaxed);
        self.saw_healthy_latency.store(false, Ordering::Relaxed);
        self.high_latency_streak.store(0, Ordering::Relaxed);
        self.high_latency_reopen_attempted
            .store(false, Ordering::Relaxed);
    }

    fn set_recovery_notifier(&self, notifier: RecoveryNotifier) {
        let mut guard = self
            .recovery_notifier
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        *guard = Some(notifier);
    }

    fn flag_reopen(&self, nonblocking_notify: bool) {
        let already_flagged = self.needs_reopen.swap(true, Ordering::AcqRel);
        if already_flagged {
            return;
        }
        if nonblocking_notify {
            if let Ok(guard) = self.recovery_notifier.try_lock() {
                if let Some(notify) = guard.as_ref() {
                    notify();
                }
            }
        } else {
            let notify = self
                .recovery_notifier
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            if let Some(notify) = notify {
                notify();
            }
        }
    }

    fn record_output_delay(&self, delay: Duration) {
        let delay_ms = delay.as_millis().min(u128::from(u32::MAX)) as u32;
        self.last_output_delay_ms.store(delay_ms, Ordering::Relaxed);

        if delay_ms <= HIGH_LATENCY_RESET_THRESHOLD_MS {
            self.saw_healthy_latency.store(true, Ordering::Relaxed);
            self.high_latency_streak.store(0, Ordering::Relaxed);
            return;
        }

        if delay_ms >= HIGH_LATENCY_REOPEN_THRESHOLD_MS
            && self.saw_healthy_latency.load(Ordering::Relaxed)
            && !self.high_latency_reopen_attempted.load(Ordering::Relaxed)
        {
            let streak = self.high_latency_streak.fetch_add(1, Ordering::Relaxed) + 1;
            if streak >= HIGH_LATENCY_REOPEN_STREAK {
                self.high_latency_reopen_attempted
                    .store(true, Ordering::Relaxed);
                self.flag_reopen(true);
            }
        } else {
            self.high_latency_streak.store(0, Ordering::Relaxed);
        }
    }

    fn handle_stream_error(&self, err: &cpal::Error) {
        match err.kind() {
            ErrorKind::DeviceChanged => {
                log::info!("Audio device rerouted; reopening stream for native rate/buffer");
                self.flag_reopen(false);
            }
            ErrorKind::StreamInvalidated => {
                log::info!("Audio stream invalidated by OS device switch; reopening");
                self.flag_reopen(false);
            }
            ErrorKind::DeviceNotAvailable => {
                log::warn!("Audio device lost ({err}); flagging immediate reopen");
                self.flag_reopen(false);
            }
            ErrorKind::Xrun => {
                let streak = self.xrun_streak.fetch_add(1, Ordering::Relaxed) + 1;
                log::debug!("Audio buffer underrun (streak={streak}): {err}");
                if streak >= XRUN_REOPEN_STREAK {
                    log::warn!("Repeated audio underruns ({streak}); flagging stream reopen");
                    self.flag_reopen(false);
                }
            }
            _ => {
                log::warn!("Audio stream error: {err}");
                self.flag_reopen(false);
            }
        }
    }

    fn volume(&self) -> f32 {
        f32::from_bits(self.volume_bits.load(Ordering::Relaxed))
    }

    fn set_volume(&self, volume: f32) {
        self.volume_bits
            .store(volume.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    fn push_voice_with_gains(&self, samples: Arc<[f32]>, gains: ChannelGains) {
        self.push_voice_at_with_gains(samples, gains, Instant::now());
    }

    fn push_voice_at_with_gains(
        &self,
        samples: Arc<[f32]>,
        gains: ChannelGains,
        queued_at: Instant,
    ) {
        if samples.len() < 2 {
            return;
        }
        let mut voices = self.voices.lock().unwrap_or_else(|e| e.into_inner());
        voices.push(Voice {
            samples,
            cursor: 0,
            left_gain: gains.left,
            right_gain: gains.right,
            queued_at,
        });
        if voices.len() > MAX_VOICES {
            let excess = voices.len() - MAX_VOICES;
            voices.drain(0..excess);
        }
        self.has_voices.store(true, Ordering::Release);
    }

    fn clear(&self) {
        let mut voices = self.voices.lock().unwrap_or_else(|e| e.into_inner());
        voices.clear();
        self.has_voices.store(false, Ordering::Release);
    }

    /// Fill `output` (`channels` interleaved samples per frame) with mixed voices.
    fn render<T>(&self, output: &mut [T], channels: usize)
    where
        T: SizedSample + FromSample<f32>,
    {
        self.callback_ticks.fetch_add(1, Ordering::Relaxed);
        self.xrun_streak.store(0, Ordering::Relaxed);

        let zero = T::from_sample(0.0);

        // Lock-free idle fast-path: skip mutex, clock read, and retain when silent.
        if !self.has_voices.load(Ordering::Acquire) {
            output.fill(zero);
            return;
        }

        let volume = self.volume();

        // Try-lock so the real-time callback never stalls behind the input thread.
        let Ok(mut voices) = self.voices.try_lock() else {
            output.fill(zero);
            return;
        };

        // Drop any unstarted voices that waited past the staleness budget so a
        // stalled/reopened audio callback never emits phantom clicks later.
        let now = Instant::now();
        voices.retain(|voice| {
            voice.cursor > 0
                || now.saturating_duration_since(voice.queued_at) <= MAX_VOICE_STALENESS
        });

        if voices.is_empty() || volume <= 0.0 {
            if !voices.is_empty() && volume <= 0.0 {
                // Still advance voices when muted so unmuting does not replay old presses.
                let frames = output.len() / channels.max(1);
                advance_silently(&mut voices, frames);
            }
            if voices.is_empty() {
                self.has_voices.store(false, Ordering::Release);
            }
            output.fill(zero);
            return;
        }

        let ch = channels.max(1);
        let apply_pan = ch >= 2;
        for frame in output.chunks_mut(ch) {
            let mut left = 0.0f32;
            let mut right = 0.0f32;

            for voice in voices.iter_mut() {
                if voice.cursor + 1 < voice.samples.len() {
                    let s_left = voice.samples[voice.cursor];
                    let s_right = voice.samples[voice.cursor + 1];
                    if apply_pan {
                        left += s_left * voice.left_gain;
                        right += s_right * voice.right_gain;
                    } else {
                        left += s_left;
                        right += s_right;
                    }
                    voice.cursor += 2;
                }
            }

            left = (left * volume).clamp(-1.0, 1.0);
            right = (right * volume).clamp(-1.0, 1.0);
            write_frame(frame, left, right);
        }

        voices.retain(|voice| voice.cursor + 1 < voice.samples.len());
        if voices.is_empty() {
            self.has_voices.store(false, Ordering::Release);
        }
    }
}

fn advance_silently(voices: &mut Vec<Voice>, frames: usize) {
    let advance = frames * 2;
    for voice in voices.iter_mut() {
        voice.cursor = voice.cursor.saturating_add(advance);
    }
    voices.retain(|voice| voice.cursor + 1 < voice.samples.len());
}

fn write_frame<T>(frame: &mut [T], left: f32, right: f32)
where
    T: SizedSample + FromSample<f32>,
{
    match frame.len() {
        0 => {}
        1 => frame[0] = T::from_sample((left + right) * 0.5),
        _ => {
            frame[0] = T::from_sample(left);
            frame[1] = T::from_sample(right);
            for extra in &mut frame[2..] {
                *extra = T::from_sample(0.0);
            }
        }
    }
}

/// Active output stream + shared mixer state.
///
/// The `cpal::Stream` is held on the thread that created it (or protected by a
/// mutex if accessed across threads — we wrap it in a dedicated struct so the
/// daemon owns it cleanly).
pub struct AudioEngine {
    shared: Arc<MixerShared>,
    stream: Mutex<Option<ActiveStream>>,
    sample_rate: AtomicU32,
}

struct ActiveStream {
    // Kept alive for the lifetime of playback.
    _stream: Stream,
    device_id: Option<DeviceId>,
    last_observed_ticks: u64,
    last_tick_advance: Instant,
}

// SAFETY: `ActiveStream` is only ever constructed, dropped, or replaced while
// holding `AudioEngine.stream`'s mutex; the underlying cpal `Stream` is never
// accessed concurrently.
unsafe impl Send for ActiveStream {}

impl AudioEngine {
    pub fn new(initial_volume: f32) -> Self {
        Self {
            shared: Arc::new(MixerShared::new(initial_volume)),
            stream: Mutex::new(None),
            sample_rate: AtomicU32::new(48_000),
        }
    }

    /// Query the default output device's preferred sample rate without opening
    /// a live playback stream (so startup pack decoding never starves audio).
    pub fn preferred_sample_rate() -> Option<u32> {
        let host = cpal::default_host();
        let device = pick_output_device(&host).ok()?;
        let supported = device.default_output_config().ok()?;
        Some(supported.sample_rate())
    }

    /// Register a callback invoked whenever the audio backend flags that the
    /// output stream should be reopened (device change, disconnect, xruns).
    pub fn set_recovery_notifier(&self, notifier: RecoveryNotifier) {
        self.shared.set_recovery_notifier(notifier);
    }

    /// Open the default audio output stream.
    pub fn init(&self) -> Result<()> {
        let mut guard = self.stream.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_some() {
            return Ok(());
        }
        self.shared.reset_stream_health();
        let (active, rate) = open_output_stream(Arc::clone(&self.shared))?;
        self.sample_rate.store(rate, Ordering::Relaxed);
        *guard = Some(active);
        Ok(())
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate.load(Ordering::Relaxed)
    }

    pub fn volume(&self) -> f32 {
        self.shared.volume()
    }

    pub fn set_volume(&self, volume: f32) {
        self.shared.set_volume(volume);
    }

    /// Return `true` if the audio callback or error handler requested a reopen.
    pub fn needs_reopen(&self) -> bool {
        self.shared.needs_reopen.load(Ordering::Acquire)
    }

    /// Latest stream playback delay reported by `cpal` in milliseconds.
    pub fn output_delay_ms(&self) -> u32 {
        self.shared.last_output_delay_ms.load(Ordering::Relaxed)
    }

    /// Reset the callback stall timer (e.g. after initial soundpack decoding completes).
    pub fn reset_watchdog(&self) {
        let ticks = self.shared.callback_ticks.load(Ordering::Relaxed);
        let mut guard = self.stream.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(active) = guard.as_mut() {
            active.last_observed_ticks = ticks;
            active.last_tick_advance = Instant::now();
        }
    }

    /// Queue a decoded clip for immediate unpanned playback without blocking the input thread.
    ///
    /// If the stream is currently flagged for recovery (e.g. mid-device-switch),
    /// drops the clip immediately so stale clicks never queue onto a dead stream.
    pub fn play_clip(&self, clip: &SoundClip) {
        self.play_clip_with_gains(clip, ChannelGains::UNCHANGED);
    }

    /// Queue a decoded clip with explicit per-voice stereo channel gains.
    pub fn play_clip_with_gains(&self, clip: &SoundClip, gains: ChannelGains) {
        if self.shared.needs_reopen.load(Ordering::Acquire) {
            return;
        }
        let target_rate = self.sample_rate();
        let samples = if clip.sample_rate == target_rate {
            Arc::clone(&clip.samples)
        } else {
            Arc::from(resample_stereo(
                &clip.samples,
                clip.sample_rate,
                target_rate,
            ))
        };
        self.shared.push_voice_with_gains(samples, gains);
    }

    /// Queue a decoded clip positioned for `zone` when `stereo_enabled` is true.
    pub fn play_clip_for_zone(&self, clip: &SoundClip, zone: KeyZone, stereo_enabled: bool) {
        self.play_clip_with_gains(clip, ChannelGains::for_zone(zone, stereo_enabled));
    }

    /// Check whether the audio callback has stopped advancing or the default
    /// output device changed, reopening the stream if needed.
    ///
    /// Returns `true` if the stream was successfully reopened (callers should
    /// check [`Self::sample_rate`] in case the new device uses a different rate).
    pub fn check_device_change(&self) -> bool {
        if self.shared.needs_reopen.swap(false, Ordering::AcqRel) || self.is_stream_stalled() {
            log::info!("Audio stream stalled or flagged for recovery; reopening");
            return self.reopen_if_needed();
        }
        let Some(current_id) = preferred_device_id() else {
            return false;
        };
        let changed = {
            let guard = self.stream.lock().unwrap_or_else(|e| e.into_inner());
            match guard.as_ref().and_then(|active| active.device_id.as_ref()) {
                Some(prev) => prev != &current_id,
                None => false,
            }
        };
        if changed {
            log::info!("Audio output device changed; reopening stream");
            return self.reopen_if_needed();
        }
        false
    }

    fn is_stream_stalled(&self) -> bool {
        let now = Instant::now();
        let ticks = self.shared.callback_ticks.load(Ordering::Relaxed);
        let mut guard = self.stream.lock().unwrap_or_else(|e| e.into_inner());
        let Some(active) = guard.as_mut() else {
            return false;
        };
        if ticks != active.last_observed_ticks {
            active.last_observed_ticks = ticks;
            active.last_tick_advance = now;
            return false;
        }
        now.saturating_duration_since(active.last_tick_advance) >= CALLBACK_STALL_TIMEOUT
    }

    fn reopen_if_needed(&self) -> bool {
        let mut guard = self.stream.lock().unwrap_or_else(|e| e.into_inner());
        self.shared.clear();
        *guard = None;
        self.shared.reset_stream_health();
        match open_output_stream(Arc::clone(&self.shared)) {
            Ok((active, rate)) => {
                self.sample_rate.store(rate, Ordering::Relaxed);
                *guard = Some(active);
                true
            }
            Err(err) => {
                log::warn!("Failed to reopen audio stream: {err}");
                self.shared.needs_reopen.store(true, Ordering::Release);
                false
            }
        }
    }

    pub fn shutdown(&self) {
        self.shared.clear();
        let mut guard = self.stream.lock().unwrap_or_else(|e| e.into_inner());
        *guard = None;
    }
}

fn preferred_device_id() -> Option<DeviceId> {
    let host = cpal::default_host();
    pick_output_device(&host)
        .ok()
        .and_then(|device| device.id().ok())
}

fn open_output_stream(shared: Arc<MixerShared>) -> Result<(ActiveStream, u32)> {
    let host = cpal::default_host();
    let device = pick_output_device(&host)?;
    let device_id = device.id().ok();
    let device_label = device
        .description()
        .map(|desc| desc.name().to_owned())
        .unwrap_or_else(|_| "default".to_owned());

    let supported = device
        .default_output_config()
        .map_err(|err| Error::new(format!("No default output config: {err}")))?;
    let format = supported.sample_format();
    let candidates = candidate_stream_configs(&supported);

    let initial_ticks = shared.callback_ticks.load(Ordering::Relaxed);
    let mut last_err = None;
    let mut built = None;
    for config in candidates {
        match build_stream(&device, config, format, Arc::clone(&shared)) {
            Ok(stream) => {
                built = Some((stream, config));
                break;
            }
            Err(err) => {
                log::debug!(
                    "Audio config buffer={:?} rejected ({err}); trying next fallback",
                    config.buffer_size
                );
                last_err = Some(err);
            }
        }
    }
    let (stream, used_config) = built.ok_or_else(|| {
        last_err.unwrap_or_else(|| Error::new("Could not build audio output stream"))
    })?;

    stream
        .play()
        .map_err(|err| Error::new(format!("Could not start audio stream: {err}")))?;

    let negotiated_frames = stream.buffer_size().ok();
    log::info!(
        "Audio output ready (device={}, rate={}, channels={}, requested={:?}, period={:?})",
        device_label,
        used_config.sample_rate,
        used_config.channels,
        used_config.buffer_size,
        negotiated_frames,
    );

    Ok((
        ActiveStream {
            _stream: stream,
            device_id,
            last_observed_ticks: initial_ticks,
            last_tick_advance: Instant::now(),
        },
        used_config.sample_rate,
    ))
}

fn pick_output_device(host: &cpal::Host) -> Result<Device> {
    // Use the system default output directly first — on Linux this opens ALSA's
    // `default` PCM (PipeWire / PulseAudio) without enumerating & probing every
    // hardware card/HDMI node; on Windows/macOS it follows the active output.
    if let Some(default) = host.default_output_device() {
        return Ok(default);
    }

    #[cfg(target_os = "linux")]
    if let Ok(devices) = host.output_devices() {
        let candidates: Vec<Device> = devices.collect();
        for preferred in ["pipewire", "pulse", "default"] {
            if let Some(dev) = candidates.iter().find(|dev| {
                dev.description()
                    .map(|d| d.name().eq_ignore_ascii_case(preferred))
                    .unwrap_or(false)
            }) {
                return Ok(dev.clone());
            }
        }
    }

    Err(Error::new("No audio output device available"))
}

/// Build an ordered low-latency buffer ladder for the active OS and device.
///
/// Trying stepped fixed sizes before `BufferSize::Default` prevents wired USB
/// headsets and Bluetooth earbuds/headphones from falling straight back to
/// multi-hundred-millisecond or multi-second default ring buffers whenever an
/// aggressive sub-6ms size is rejected.
fn candidate_stream_configs(supported: &SupportedStreamConfig) -> Vec<StreamConfig> {
    candidate_stream_configs_for_platform(supported, cfg!(target_os = "linux"))
}

fn candidate_stream_configs_for_platform(
    supported: &SupportedStreamConfig,
    is_linux: bool,
) -> Vec<StreamConfig> {
    let base = supported.config();
    let targets = preferred_period_frames(base.sample_rate, is_linux);

    let mut sizes: Vec<BufferSize> = Vec::with_capacity(targets.len() + 1);
    for target in targets {
        let candidate = match supported.buffer_size() {
            SupportedBufferSize::Range { min, max } => BufferSize::Fixed(target.clamp(*min, *max)),
            SupportedBufferSize::Unknown => BufferSize::Fixed(target),
        };
        if !sizes.contains(&candidate) {
            sizes.push(candidate);
        }
    }
    sizes.push(BufferSize::Default);

    sizes
        .into_iter()
        .map(|buffer_size| {
            let mut cfg = base;
            cfg.buffer_size = buffer_size;
            cfg
        })
        .collect()
}

/// Sample-rate-scaled low-latency period frame targets.
///
/// - On Linux we start at ~10.7 ms (`512` @ 48 kHz) rather than `128`/`256` to
///   avoid hardware EPIPE loops on Intel SOF codecs and Bluetooth A2DP sinks,
///   while still bounding PipeWire/PulseAudio/ALSA ring buffers to 2 periods
///   (~21–43 ms) instead of letting unconstrained `Default` balloon to seconds.
/// - On Windows/macOS we start at ~5.3 ms (`256` @ 48 kHz) and step cleanly to
///   ~10.7 ms (`512`) and ~21.3 ms (`1024`) for Bluetooth / USB headsets whose
///   minimum buffer exceeds 256 frames.
fn preferred_period_frames(sample_rate: u32, is_linux: bool) -> Vec<u32> {
    let scale_pow2 = |ms_num: u64, min_frames: u32| -> u32 {
        let raw = (u64::from(sample_rate.max(8_000)) * ms_num) / 1000;
        let pow2 = (raw.max(u64::from(min_frames)) as u32).next_power_of_two();
        // Snap downward if the previous power of two is closer to the target duration.
        let prev = pow2 / 2;
        if prev >= min_frames && raw.abs_diff(u64::from(prev)) <= u64::from(pow2).abs_diff(raw) {
            prev
        } else {
            pow2
        }
    };

    let raw_targets: &[u32] = if is_linux {
        &[scale_pow2(11, 256), scale_pow2(21, 512)]
    } else {
        &[scale_pow2(5, 128), scale_pow2(11, 256), scale_pow2(21, 512)]
    };

    let mut deduped = Vec::with_capacity(raw_targets.len());
    for &frames in raw_targets {
        if !deduped.contains(&frames) {
            deduped.push(frames);
        }
    }
    deduped
}

fn build_stream(
    device: &Device,
    config: StreamConfig,
    format: SampleFormat,
    shared: Arc<MixerShared>,
) -> Result<Stream> {
    match format {
        SampleFormat::F32 => build_typed_stream::<f32>(device, config, shared),
        SampleFormat::F64 => build_typed_stream::<f64>(device, config, shared),
        SampleFormat::I16 => build_typed_stream::<i16>(device, config, shared),
        SampleFormat::I32 => build_typed_stream::<i32>(device, config, shared),
        SampleFormat::U16 => build_typed_stream::<u16>(device, config, shared),
        SampleFormat::U32 => build_typed_stream::<u32>(device, config, shared),
        SampleFormat::I8 => build_typed_stream::<i8>(device, config, shared),
        SampleFormat::U8 => build_typed_stream::<u8>(device, config, shared),
        other => Err(Error::new(format!(
            "Unsupported audio sample format: {other}"
        ))),
    }
}

fn build_typed_stream<T>(
    device: &Device,
    config: StreamConfig,
    shared: Arc<MixerShared>,
) -> Result<Stream>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = usize::from(config.channels);
    let data_shared = Arc::clone(&shared);
    let err_shared = shared;

    device
        .build_output_stream(
            config,
            move |data: &mut [T], info| {
                let ts = info.timestamp();
                let delay = ts.playback.duration_since(ts.callback);
                data_shared.record_output_delay(delay);
                data_shared.render(data, channels);
            },
            move |err| err_shared.handle_stream_error(&err),
            Some(STREAM_POLL_TIMEOUT),
        )
        .map_err(|err| Error::new(format!("Could not build audio output stream: {err}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn mixer_drops_stale_unstarted_voices_to_prevent_phantom_clicks() {
        let mixer = MixerShared::new(1.0);
        let stale_time = Instant::now() - Duration::from_millis(120);
        mixer.push_voice_at_with_gains(
            Arc::from(vec![0.9f32, 0.9, 0.9, 0.9]),
            ChannelGains::UNCHANGED,
            stale_time,
        );

        let mut buf = [0.0f32; 4];
        mixer.render(&mut buf, 2);
        assert_eq!(buf, [0.0, 0.0, 0.0, 0.0]);
        assert!(mixer.voices.lock().unwrap().is_empty());
    }

    #[test]
    fn mixer_combines_overlapping_voices_and_clamps() {
        let mixer = MixerShared::new(0.5);
        mixer.push_voice_with_gains(
            Arc::from(vec![0.8f32, 0.4, 0.2, 0.2]),
            ChannelGains::UNCHANGED,
        );
        mixer.push_voice_with_gains(Arc::from(vec![0.8f32, 0.4]), ChannelGains::UNCHANGED);

        let mut buf = [0.0f32; 6]; // 3 stereo frames
        mixer.render(&mut buf, 2);

        // Frame 0: (0.8+0.8)*0.5 = 0.8, (0.4+0.4)*0.5 = 0.4
        assert!((buf[0] - 0.8).abs() < 1e-5);
        assert!((buf[1] - 0.4).abs() < 1e-5);
        // Frame 1: only voice 1 remains: 0.2*0.5 = 0.1
        assert!((buf[2] - 0.1).abs() < 1e-5);
        assert!((buf[3] - 0.1).abs() < 1e-5);
        // Frame 2: silence
        assert_eq!(buf[4], 0.0);
        assert_eq!(buf[5], 0.0);
    }

    #[test]
    fn mixer_caps_voice_count() {
        let mixer = MixerShared::new(1.0);
        for _ in 0..20 {
            mixer.push_voice_with_gains(
                Arc::from(vec![0.1f32, 0.1, 0.1, 0.1]),
                ChannelGains::UNCHANGED,
            );
        }
        assert_eq!(mixer.voices.lock().unwrap().len(), MAX_VOICES);
    }

    #[test]
    fn stereo_channel_gains_match_zones_and_disable_cleanly() {
        assert_eq!(
            ChannelGains::for_zone(KeyZone::Left, true),
            ChannelGains {
                left: 1.0,
                right: 0.24
            }
        );
        assert_eq!(
            ChannelGains::for_zone(KeyZone::Center, true),
            ChannelGains::UNCHANGED
        );
        assert_eq!(
            ChannelGains::for_zone(KeyZone::Right, true),
            ChannelGains {
                left: 0.24,
                right: 1.0
            }
        );
        for zone in [KeyZone::Left, KeyZone::Center, KeyZone::Right] {
            assert_eq!(ChannelGains::for_zone(zone, false), ChannelGains::UNCHANGED);
        }
    }

    #[test]
    fn mixer_renders_asymmetric_and_overlapping_stereo_voices() {
        // Mono clip stored as interleaved stereo (L == R == 0.5).
        let mono_clip: Arc<[f32]> = Arc::from(vec![0.5f32, 0.5, 0.5, 0.5]);

        // Left key (A): favors left channel (0.50 vs 0.12).
        let mixer = MixerShared::new(1.0);
        mixer.push_voice_with_gains(
            Arc::clone(&mono_clip),
            ChannelGains::for_zone(KeyZone::Left, true),
        );
        let mut left_buf = [0.0f32; 4];
        mixer.render(&mut left_buf, 2);
        assert!((left_buf[0] - 0.5).abs() < 1e-5);
        assert!((left_buf[1] - 0.12).abs() < 1e-5);
        assert!(left_buf[0] > left_buf[1]);

        // Center key (G/H): balanced on both channels (0.50 vs 0.50).
        mixer.push_voice_with_gains(
            Arc::clone(&mono_clip),
            ChannelGains::for_zone(KeyZone::Center, true),
        );
        let mut center_buf = [0.0f32; 4];
        mixer.render(&mut center_buf, 2);
        assert!((center_buf[0] - 0.5).abs() < 1e-5);
        assert!((center_buf[1] - 0.5).abs() < 1e-5);

        // Right key (L): favors right channel (0.12 vs 0.50).
        mixer.push_voice_with_gains(
            Arc::clone(&mono_clip),
            ChannelGains::for_zone(KeyZone::Right, true),
        );
        let mut right_buf = [0.0f32; 4];
        mixer.render(&mut right_buf, 2);
        assert!((right_buf[0] - 0.12).abs() < 1e-5);
        assert!((right_buf[1] - 0.5).abs() < 1e-5);
        assert!(right_buf[1] > right_buf[0]);

        // Overlapping A (0.6) + L (0.2) retain distinct positions simultaneously:
        // Left channel = 0.6*1.0 + 0.2*0.24 = 0.648; Right = 0.6*0.24 + 0.2*1.0 = 0.344.
        mixer.push_voice_with_gains(
            Arc::from(vec![0.6f32, 0.6]),
            ChannelGains::for_zone(KeyZone::Left, true),
        );
        mixer.push_voice_with_gains(
            Arc::from(vec![0.2f32, 0.2]),
            ChannelGains::for_zone(KeyZone::Right, true),
        );
        let mut overlap = [0.0f32; 2];
        mixer.render(&mut overlap, 2);
        assert!((overlap[0] - 0.648).abs() < 1e-5);
        assert!((overlap[1] - 0.344).abs() < 1e-5);
    }

    #[test]
    fn mixer_uses_unpanned_downmix_when_rendering_to_mono_output() {
        let clip: Arc<[f32]> = Arc::from(vec![0.8f32, 0.4, 0.2, 0.6]);

        let panned = MixerShared::new(0.5);
        panned.push_voice_with_gains(
            Arc::clone(&clip),
            ChannelGains::for_zone(KeyZone::Left, true),
        );
        let mut panned_mono = [0.0f32; 2];
        panned.render(&mut panned_mono, 1);

        let unpanned = MixerShared::new(0.5);
        unpanned.push_voice_with_gains(clip, ChannelGains::UNCHANGED);
        let mut unpanned_mono = [0.0f32; 2];
        unpanned.render(&mut unpanned_mono, 1);

        assert_eq!(panned_mono, unpanned_mono);
        // Frame 0: ((0.8 + 0.4) * 0.5) * 0.5 = 0.3; Frame 1: ((0.2 + 0.6) * 0.5) * 0.5 = 0.2
        assert!((panned_mono[0] - 0.3).abs() < 1e-5);
        assert!((panned_mono[1] - 0.2).abs() < 1e-5);
    }

    #[test]
    fn buffer_ladder_steps_cleanly_across_desktop_os_profiles() {
        // Desktop Linux @ 48 kHz with unconstrained PipeWire/ALSA range.
        let linux_48k = SupportedStreamConfig::new(
            2,
            48_000,
            SupportedBufferSize::Range {
                min: 32,
                max: 16_384,
            },
            SampleFormat::F32,
        );
        let linux_sizes: Vec<BufferSize> = candidate_stream_configs_for_platform(&linux_48k, true)
            .into_iter()
            .map(|c| c.buffer_size)
            .collect();
        assert_eq!(
            linux_sizes,
            vec![
                BufferSize::Fixed(512),
                BufferSize::Fixed(1024),
                BufferSize::Default,
            ]
        );

        // Windows/macOS Bluetooth headset @ 48 kHz with higher minimum buffer (480..4096).
        let bt_headset = SupportedStreamConfig::new(
            2,
            48_000,
            SupportedBufferSize::Range {
                min: 480,
                max: 4096,
            },
            SampleFormat::F32,
        );
        let bt_sizes: Vec<BufferSize> = candidate_stream_configs_for_platform(&bt_headset, false)
            .into_iter()
            .map(|c| c.buffer_size)
            .collect();
        assert_eq!(
            bt_sizes,
            vec![
                BufferSize::Fixed(480),
                BufferSize::Fixed(512),
                BufferSize::Fixed(1024),
                BufferSize::Default,
            ]
        );

        // Windows WASAPI software stack with Unknown buffer limits still tries fixed low latency.
        let wasapi_unknown =
            SupportedStreamConfig::new(2, 48_000, SupportedBufferSize::Unknown, SampleFormat::F32);
        let unknown_sizes: Vec<BufferSize> =
            candidate_stream_configs_for_platform(&wasapi_unknown, false)
                .into_iter()
                .map(|c| c.buffer_size)
                .collect();
        assert_eq!(
            unknown_sizes,
            vec![
                BufferSize::Fixed(256),
                BufferSize::Fixed(512),
                BufferSize::Fixed(1024),
                BufferSize::Default,
            ]
        );

        // Bluetooth HFP 16 kHz hands-free mode scales targets down proportionally.
        let bt_hfp_16k = SupportedStreamConfig::new(
            1,
            16_000,
            SupportedBufferSize::Range { min: 64, max: 2048 },
            SampleFormat::I16,
        );
        let hfp_sizes: Vec<BufferSize> = candidate_stream_configs_for_platform(&bt_hfp_16k, false)
            .into_iter()
            .map(|c| c.buffer_size)
            .collect();
        assert_eq!(
            hfp_sizes,
            vec![
                BufferSize::Fixed(128),
                BufferSize::Fixed(256),
                BufferSize::Fixed(512),
                BufferSize::Default,
            ]
        );
    }

    #[test]
    fn device_change_and_stream_invalidation_trigger_immediate_recovery() {
        let mixer = MixerShared::new(1.0);
        let wakeups = Arc::new(AtomicUsize::new(0));
        let wakeups_clone = Arc::clone(&wakeups);
        mixer.set_recovery_notifier(Arc::new(move || {
            wakeups_clone.fetch_add(1, Ordering::Relaxed);
        }));

        // macOS CoreAudio headphone plug/Bluetooth reroute.
        mixer.handle_stream_error(&cpal::Error::from(ErrorKind::DeviceChanged));
        assert!(mixer.needs_reopen.load(Ordering::Acquire));
        assert_eq!(wakeups.load(Ordering::Relaxed), 1);

        // Windows WASAPI default endpoint change.
        mixer.reset_stream_health();
        mixer.handle_stream_error(&cpal::Error::from(ErrorKind::StreamInvalidated));
        assert!(mixer.needs_reopen.load(Ordering::Acquire));
        assert_eq!(wakeups.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn play_clip_drops_voices_while_reopen_is_pending() {
        let engine = AudioEngine::new(1.0);
        engine.shared.needs_reopen.store(true, Ordering::Release);
        let clip = SoundClip {
            samples: Arc::from(vec![0.5f32, 0.5, 0.5, 0.5]),
            sample_rate: 48_000,
        };
        engine.play_clip(&clip);
        assert!(engine.shared.voices.lock().unwrap().is_empty());
    }

    #[test]
    fn runaway_ring_buffer_latency_triggers_single_reopen() {
        let mixer = MixerShared::new(1.0);
        let wakeups = Arc::new(AtomicUsize::new(0));
        let wakeups_clone = Arc::clone(&wakeups);
        mixer.set_recovery_notifier(Arc::new(move || {
            wakeups_clone.fetch_add(1, Ordering::Relaxed);
        }));

        // Start healthy (25 ms), then spike to 750 ms after a bad handoff.
        mixer.record_output_delay(Duration::from_millis(25));
        for _ in 0..HIGH_LATENCY_REOPEN_STREAK {
            mixer.record_output_delay(Duration::from_millis(750));
        }
        assert!(mixer.needs_reopen.load(Ordering::Acquire));
        assert_eq!(wakeups.load(Ordering::Relaxed), 1);

        // A device that always reports high hardware latency from frame 1 never thrashes.
        mixer.reset_stream_health();
        for _ in 0..(HIGH_LATENCY_REOPEN_STREAK * 2) {
            mixer.record_output_delay(Duration::from_millis(600));
        }
        assert!(!mixer.needs_reopen.load(Ordering::Acquire));
    }
}
