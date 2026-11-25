//! Low-latency overlapping keystroke mixer backed by `cpal`.
//!
//! - Uses PipeWire/PulseAudio low-latency environment hints on Linux, WASAPI on
//!   Windows, and CoreAudio on macOS.
//! - Mixes up to `MAX_VOICES` overlapping stereo clips inside the audio callback
//!   with zero heap allocation on the real-time thread.
//! - Automatically reopens the output stream if the default output device
//!   changes or disconnects.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{
    BufferSize, Device, DeviceId, ErrorKind, FromSample, SampleFormat, SizedSample, Stream,
    StreamConfig, SupportedBufferSize, SupportedStreamConfig,
};

use crate::soundpack::{resample_stereo, SoundClip};
use crate::{Error, Result};

const TARGET_BUFFER_FRAMES: u32 = 128;
const MAX_VOICES: usize = 8;

/// Hint PipeWire / PulseAudio toward a low-latency quantum (Linux only).
///
/// Must be called early during daemon startup before opening audio streams.
pub fn apply_low_latency_env() {
    #[cfg(target_os = "linux")]
    {
        if std::env::var_os("PIPEWIRE_LATENCY").is_none() {
            std::env::set_var("PIPEWIRE_LATENCY", "64/48000");
        }
        if std::env::var_os("PULSE_LATENCY_MSEC").is_none() {
            std::env::set_var("PULSE_LATENCY_MSEC", "8");
        }
        if std::env::var_os("PULSE_PROP_media.role").is_none() {
            std::env::set_var("PULSE_PROP_media.role", "event");
        }
        let existing = std::env::var("PIPEWIRE_PROPS").unwrap_or_default();
        if !existing.contains("node.latency=") {
            let props = format!("{existing} media.role=Event node.latency=64/48000");
            std::env::set_var("PIPEWIRE_PROPS", props.trim());
        }
    }
}

struct Voice {
    /// Interleaved stereo `f32` samples matching the stream's sample rate.
    samples: Arc<[f32]>,
    /// Current index into `samples` (always even, advances by 2 per frame).
    cursor: usize,
}

struct MixerShared {
    voices: Mutex<Vec<Voice>>,
    /// Volume as `f32::to_bits()`, clamped to `[0.0, 1.0]`.
    volume_bits: AtomicU32,
    needs_reopen: AtomicBool,
}

impl MixerShared {
    fn new(volume: f32) -> Self {
        Self {
            voices: Mutex::new(Vec::with_capacity(MAX_VOICES)),
            volume_bits: AtomicU32::new(volume.clamp(0.0, 1.0).to_bits()),
            needs_reopen: AtomicBool::new(false),
        }
    }

    fn volume(&self) -> f32 {
        f32::from_bits(self.volume_bits.load(Ordering::Relaxed))
    }

    fn set_volume(&self, volume: f32) {
        self.volume_bits
            .store(volume.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    fn push_voice(&self, samples: Arc<[f32]>) {
        if samples.len() < 2 {
            return;
        }
        let mut voices = self.voices.lock().unwrap_or_else(|e| e.into_inner());
        voices.push(Voice { samples, cursor: 0 });
        if voices.len() > MAX_VOICES {
            let excess = voices.len() - MAX_VOICES;
            voices.drain(0..excess);
        }
    }

    fn clear(&self) {
        let mut voices = self.voices.lock().unwrap_or_else(|e| e.into_inner());
        voices.clear();
    }

    /// Fill `output` (`channels` interleaved samples per frame) with mixed voices.
    fn render<T>(&self, output: &mut [T], channels: usize)
    where
        T: SizedSample + FromSample<f32>,
    {
        let zero = T::from_sample(0.0);
        let volume = self.volume();

        // Try-lock so the real-time callback never stalls behind the input thread.
        let Ok(mut voices) = self.voices.try_lock() else {
            output.fill(zero);
            return;
        };

        if voices.is_empty() || volume <= 0.0 {
            if !voices.is_empty() && volume <= 0.0 {
                // Still advance voices when muted so unmuting does not replay old presses.
                let frames = output.len() / channels.max(1);
                advance_silently(&mut voices, frames);
            }
            output.fill(zero);
            return;
        }

        let ch = channels.max(1);
        for frame in output.chunks_mut(ch) {
            let mut left = 0.0f32;
            let mut right = 0.0f32;

            for voice in voices.iter_mut() {
                if voice.cursor + 1 < voice.samples.len() {
                    left += voice.samples[voice.cursor];
                    right += voice.samples[voice.cursor + 1];
                    voice.cursor += 2;
                }
            }

            left = (left * volume).clamp(-1.0, 1.0);
            right = (right * volume).clamp(-1.0, 1.0);
            write_frame(frame, left, right);
        }

        voices.retain(|voice| voice.cursor + 1 < voice.samples.len());
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

    /// Open the default audio output stream.
    pub fn init(&self) -> Result<()> {
        let mut guard = self.stream.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_some() {
            return Ok(());
        }
        let (active, rate) = open_output_stream(Arc::clone(&self.shared))?;
        self.sample_rate.store(rate, Ordering::Relaxed);
        self.shared.needs_reopen.store(false, Ordering::Relaxed);
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

    /// Queue a decoded clip for immediate playback.
    pub fn play_clip(&self, clip: &SoundClip) {
        if self.shared.needs_reopen.swap(false, Ordering::AcqRel) {
            self.reopen_if_needed();
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
        self.shared.push_voice(samples);
    }

    /// Check whether the system default audio device changed and reopen if so.
    pub fn check_device_change(&self) {
        if self.shared.needs_reopen.swap(false, Ordering::AcqRel) {
            self.reopen_if_needed();
            return;
        }
        let Some(current_id) = default_device_id() else {
            return;
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
            self.reopen_if_needed();
        }
    }

    fn reopen_if_needed(&self) {
        let mut guard = self.stream.lock().unwrap_or_else(|e| e.into_inner());
        self.shared.clear();
        *guard = None;
        match open_output_stream(Arc::clone(&self.shared)) {
            Ok((active, rate)) => {
                self.sample_rate.store(rate, Ordering::Relaxed);
                *guard = Some(active);
            }
            Err(err) => {
                log::warn!("Failed to reopen audio stream: {err}");
                self.shared.needs_reopen.store(true, Ordering::Release);
            }
        }
    }

    pub fn shutdown(&self) {
        self.shared.clear();
        let mut guard = self.stream.lock().unwrap_or_else(|e| e.into_inner());
        *guard = None;
    }
}

fn default_device_id() -> Option<DeviceId> {
    let host = cpal::default_host();
    host.default_output_device()
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
    let preferred_config = choose_stream_config(&supported);
    let fallback_config = supported.config();

    let (stream, used_config) =
        match build_stream(&device, preferred_config, format, Arc::clone(&shared)) {
            Ok(stream) => (stream, preferred_config),
            Err(first_err) => {
                log::debug!(
                    "Low-latency config failed ({first_err}); falling back to default buffer"
                );
                let stream = build_stream(&device, fallback_config, format, shared)?;
                (stream, fallback_config)
            }
        };

    stream
        .play()
        .map_err(|err| Error::new(format!("Could not start audio stream: {err}")))?;

    log::info!(
        "Audio output ready (device={}, rate={}, channels={}, buffer={:?})",
        device_label,
        used_config.sample_rate,
        used_config.channels,
        used_config.buffer_size,
    );

    Ok((
        ActiveStream {
            _stream: stream,
            device_id,
        },
        used_config.sample_rate,
    ))
}

fn pick_output_device(host: &cpal::Host) -> Result<Device> {
    #[cfg(target_os = "linux")]
    {
        // Prefer PipeWire / PulseAudio / default ALSA aliases just like Mechy.
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
    }

    host.default_output_device()
        .ok_or_else(|| Error::new("No audio output device available"))
}

fn choose_stream_config(supported: &SupportedStreamConfig) -> StreamConfig {
    let mut config = supported.config();
    config.buffer_size = match supported.buffer_size() {
        SupportedBufferSize::Range { min, max } => {
            BufferSize::Fixed(TARGET_BUFFER_FRAMES.clamp(*min, *max))
        }
        SupportedBufferSize::Unknown => BufferSize::Default,
    };
    config
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
            move |data: &mut [T], _| data_shared.render(data, channels),
            move |err| match err.kind() {
                ErrorKind::DeviceChanged => {
                    log::info!("Audio device rerouted automatically");
                }
                ErrorKind::DeviceNotAvailable => {
                    log::warn!("Audio device lost ({err}); will reopen on next keystroke");
                    err_shared.needs_reopen.store(true, Ordering::Release);
                }
                ErrorKind::Xrun => {
                    log::debug!("Audio buffer underrun: {err}");
                }
                _ => {
                    log::warn!("Audio stream error: {err}");
                    err_shared.needs_reopen.store(true, Ordering::Release);
                }
            },
            None,
        )
        .map_err(|err| Error::new(format!("Could not build audio output stream: {err}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixer_combines_overlapping_voices_and_clamps() {
        let mixer = MixerShared::new(0.5);
        mixer.push_voice(Arc::from(vec![0.8f32, 0.4, 0.2, 0.2]));
        mixer.push_voice(Arc::from(vec![0.8f32, 0.4]));

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
            mixer.push_voice(Arc::from(vec![0.1f32, 0.1, 0.1, 0.1]));
        }
        assert_eq!(mixer.voices.lock().unwrap().len(), MAX_VOICES);
    }
}
