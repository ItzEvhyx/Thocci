//! Local GUI audio audition client.
//!
//! Plays single-shot key press and release previews for explicit dashboard
//! pointer/touch auditions, pack selection, and Quick Actions activations ONLY
//! while desktop playback is explicitly enabled and responsive — never on fresh
//! launch, during onboarding, while Off/starting, or during teardown.
//!
//! Keeps its output stream warm across an enabled typing/clicking burst
//! (avoiding repeated Bluetooth A2DP transport suspend/resume delays), closes it
//! immediately on Disable/Quit via [`PreviewPlayer::quiesce`] /
//! [`PreviewPlayer::shutdown`], and closes it after an idle period via a single
//! background reaper thread.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use thocci_core::audio::{AudioEngine, ChannelGains};
use thocci_core::keymap::{self, KeyZone};
use thocci_core::soundpack::Soundpack;

const IDLE_CLOSE_DELAY: Duration = Duration::from_secs(20);
const DEVICE_CHECK_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Default)]
struct PreviewInner {
    audio: Option<AudioEngine>,
    loaded_pack_id: Option<String>,
    loaded_pack: Option<Arc<Soundpack>>,
    last_device_check: Option<Instant>,
    last_activity: Option<Instant>,
    reaper_running: bool,
}

pub struct PreviewPlayer {
    inner: Arc<Mutex<PreviewInner>>,
    /// Explicitly opt-in: starts `false` so nothing can play before playback is enabled.
    enabled: AtomicBool,
    /// Monotonically incremented on every quiesce/shutdown so in-flight preview
    /// requests that raced with Disable/Quit abort cleanly without playing.
    generation: AtomicU64,
}

impl Default for PreviewPlayer {
    fn default() -> Self {
        Self::new()
    }
}

impl PreviewPlayer {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(PreviewInner::default())),
            enabled: AtomicBool::new(false),
            generation: AtomicU64::new(0),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }

    /// Authorize or revoke GUI preview playback to match the confirmed engine
    /// session state. Disabling immediately quiesces any active preview audio.
    pub fn set_enabled(&self, allowed: bool) {
        if allowed {
            self.enabled.store(true, Ordering::Release);
        } else {
            self.quiesce();
        }
    }

    /// Immediately disable preview playback, invalidate any in-flight preview
    /// requests, and silence/close the preview audio stream while preserving
    /// cached pack decoding for a future enabled session.
    pub fn quiesce(&self) {
        self.enabled.store(false, Ordering::Release);
        self.generation.fetch_add(1, Ordering::AcqRel);
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(engine) = guard.audio.take() {
            engine.shutdown();
        }
        guard.last_device_check = None;
        guard.last_activity = None;
    }

    /// Play one audition sample (`kind` = `"down"` or `"up"`) at `volume` (0-100),
    /// applying the shared physical-key stereo zone when `stereo` is enabled.
    ///
    /// Returns `Ok(false)` silently without opening audio when playback is not
    /// enabled, when volume is 0, or if quiesced before playback begins.
    pub fn play(
        &self,
        code: Option<&str>,
        kind: &str,
        pack_id: &str,
        volume: u8,
        stereo: bool,
    ) -> Result<bool, String> {
        if !self.is_enabled() || volume == 0 {
            return Ok(false);
        }
        let token = self.generation.load(Ordering::Acquire);

        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if !self.is_enabled() || self.generation.load(Ordering::Acquire) != token {
            return Ok(false);
        }

        let now = Instant::now();
        let normalized_vol = f32::from(volume.min(100)) / 100.0;

        // Decode the soundpack before opening a fresh audio stream so decoding
        // never starves a newly opened Bluetooth/headphone stream.
        let needs_pack_load =
            guard.loaded_pack_id.as_deref() != Some(pack_id) || guard.loaded_pack.is_none();
        if guard.audio.is_none() {
            thocci_core::audio::apply_low_latency_env();
            if needs_pack_load {
                let target_rate = AudioEngine::preferred_sample_rate();
                let pack =
                    Soundpack::load_by_id(pack_id, target_rate).map_err(|err| err.to_string())?;
                guard.loaded_pack = Some(Arc::new(pack));
                guard.loaded_pack_id = Some(pack_id.to_owned());
            }
            if !self.is_enabled() || self.generation.load(Ordering::Acquire) != token {
                return Ok(false);
            }
            let engine = AudioEngine::new(normalized_vol);
            engine.init().map_err(|err| err.to_string())?;
            guard.audio = Some(engine);
            guard.last_device_check = Some(now);
        }

        let sample_rate = {
            let needs_check = guard.audio.as_ref().is_some_and(AudioEngine::needs_reopen)
                || guard.last_device_check.is_none_or(|last| {
                    now.saturating_duration_since(last) >= DEVICE_CHECK_INTERVAL
                });
            let engine = guard.audio.as_ref().expect("audio initialized");
            engine.set_volume(normalized_vol);
            if needs_check {
                engine.check_device_change();
                guard.last_device_check = Some(now);
            }
            guard
                .audio
                .as_ref()
                .expect("audio initialized")
                .sample_rate()
        };

        let needs_reload =
            guard.loaded_pack_id.as_deref() != Some(pack_id) || guard.loaded_pack.is_none();
        if needs_reload {
            let pack =
                Soundpack::load_by_id(pack_id, Some(sample_rate)).map_err(|err| err.to_string())?;
            guard.loaded_pack = Some(Arc::new(pack));
            guard.loaded_pack_id = Some(pack_id.to_owned());
        } else if let Some(existing) = guard.loaded_pack.as_ref() {
            if existing.sample_rate() != Some(sample_rate) {
                let resampled = existing.resampled_to(sample_rate);
                guard.loaded_pack = Some(Arc::new(resampled));
            }
        }

        if !self.is_enabled() || self.generation.load(Ordering::Acquire) != token {
            if let Some(engine) = guard.audio.take() {
                engine.shutdown();
            }
            return Ok(false);
        }

        let Some(pack) = guard.loaded_pack.as_ref() else {
            return Ok(false);
        };
        let sound = match code {
            Some(dom_code) if !dom_code.is_empty() => pack.sound_for_dom_code(dom_code),
            _ => pack.representative_sound(),
        };
        let Some(sound) = sound else {
            return Ok(false);
        };

        let zone = match code {
            Some(dom_code) if !dom_code.is_empty() => keymap::dom_code_zone(dom_code),
            _ => KeyZone::Center,
        };
        let gains = ChannelGains::for_zone(zone, stereo);

        let engine = guard.audio.as_ref().expect("audio initialized");
        let played = match kind {
            "up" => {
                if let Some(release) = &sound.up {
                    engine.play_clip_with_gains(release, gains);
                    true
                } else {
                    false
                }
            }
            _ => {
                engine.play_clip_with_gains(&sound.down, gains);
                true
            }
        };

        guard.last_activity = Some(Instant::now());
        if !guard.reaper_running {
            guard.reaper_running = true;
            let inner_ref = Arc::clone(&self.inner);
            std::thread::spawn(move || loop {
                let sleep_for = {
                    let mut g = inner_ref.lock().unwrap_or_else(|e| e.into_inner());
                    if g.audio.is_none() {
                        g.reaper_running = false;
                        return;
                    }
                    let elapsed = g
                        .last_activity
                        .map(|t| t.elapsed())
                        .unwrap_or(IDLE_CLOSE_DELAY);
                    if elapsed >= IDLE_CLOSE_DELAY {
                        if let Some(engine) = g.audio.take() {
                            engine.shutdown();
                        }
                        g.last_device_check = None;
                        g.reaper_running = false;
                        return;
                    }
                    IDLE_CLOSE_DELAY.saturating_sub(elapsed)
                };
                std::thread::sleep(sleep_for);
            });
        }

        Ok(played)
    }

    /// Clear cached pack so a refreshed or replaced pack reloads on next audition.
    pub fn invalidate_cache(&self) {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.loaded_pack = None;
        guard.loaded_pack_id = None;
    }

    pub fn shutdown(&self) {
        self.enabled.store(false, Ordering::Release);
        self.generation.fetch_add(1, Ordering::AcqRel);
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(engine) = guard.audio.take() {
            engine.shutdown();
        }
        guard.loaded_pack = None;
        guard.loaded_pack_id = None;
        guard.last_device_check = None;
        guard.last_activity = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_by_default_and_silent_even_with_nonzero_volume() {
        let player = PreviewPlayer::new();
        assert!(!player.is_enabled());
        let played = player
            .play(Some("Space"), "down", "cherry-mx", 100, false)
            .expect("disabled preview returns Ok(false)");
        assert!(!played);
        assert!(player.inner.lock().unwrap().audio.is_none());
    }

    #[test]
    fn volume_zero_is_strictly_silent_without_opening_audio() {
        let player = PreviewPlayer::new();
        player.set_enabled(true);
        let played = player
            .play(Some("KeyA"), "down", "cherry-mx", 0, true)
            .expect("volume 0 succeeds silently");
        assert!(!played);
        assert!(player.inner.lock().unwrap().audio.is_none());
    }

    #[test]
    fn quiesce_disables_preview_and_is_safe_to_call_repeatedly() {
        let player = PreviewPlayer::new();
        player.set_enabled(true);
        assert!(player.is_enabled());
        player.quiesce();
        player.quiesce();
        assert!(!player.is_enabled());
        let played = player
            .play(Some("KeyA"), "down", "cherry-mx", 80, false)
            .expect("quiesced preview returns Ok(false)");
        assert!(!played);
        assert!(player.inner.lock().unwrap().audio.is_none());
    }
}
