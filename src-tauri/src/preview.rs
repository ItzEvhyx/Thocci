//! Local GUI audio audition client.
//!
//! Plays single-shot key press and release previews for explicit dashboard
//! pointer/touch auditions and Quick Actions activations using the active
//! soundpack and current volume — even while the global background daemon is
//! stopped — without spawning a second keyboard listener.
//!
//! Automatically closes its idle output stream after a few seconds of
//! inactivity so the desktop UI does not keep a second permanent PipeWire/ALSA
//! stream open alongside the background daemon.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use thocci_core::audio::AudioEngine;
use thocci_core::soundpack::Soundpack;

const IDLE_CLOSE_DELAY: Duration = Duration::from_secs(6);

#[derive(Default)]
struct PreviewInner {
    audio: Option<AudioEngine>,
    loaded_pack_id: Option<String>,
    loaded_pack: Option<Arc<Soundpack>>,
}

pub struct PreviewPlayer {
    inner: Arc<Mutex<PreviewInner>>,
    generation: Arc<AtomicU64>,
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
            generation: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Play one audition sample (`kind` = `"down"` or `"up"`) at `volume` (0-100).
    ///
    /// Volume 0 returns `Ok(false)` immediately and stays completely silent.
    pub fn play(
        &self,
        code: Option<&str>,
        kind: &str,
        pack_id: &str,
        volume: u8,
    ) -> Result<bool, String> {
        if volume == 0 {
            return Ok(false);
        }

        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());

        let normalized_vol = f32::from(volume.min(100)) / 100.0;
        if guard.audio.is_none() {
            thocci_core::audio::apply_low_latency_env();
            let engine = AudioEngine::new(normalized_vol);
            engine.init().map_err(|err| err.to_string())?;
            guard.audio = Some(engine);
        }

        let sample_rate = {
            let engine = guard.audio.as_ref().expect("audio initialized");
            engine.set_volume(normalized_vol);
            engine.check_device_change();
            engine.sample_rate()
        };

        let needs_reload =
            guard.loaded_pack_id.as_deref() != Some(pack_id) || guard.loaded_pack.is_none();
        if needs_reload {
            let pack =
                Soundpack::load_by_id(pack_id, Some(sample_rate)).map_err(|err| err.to_string())?;
            guard.loaded_pack = Some(Arc::new(pack));
            guard.loaded_pack_id = Some(pack_id.to_owned());
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

        let engine = guard.audio.as_ref().expect("audio initialized");
        let played = match kind {
            "up" => {
                if let Some(release) = &sound.up {
                    engine.play_clip(release);
                    true
                } else {
                    false
                }
            }
            _ => {
                engine.play_clip(&sound.down);
                true
            }
        };

        let next_gen = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        let inner_ref = Arc::clone(&self.inner);
        let gen_ref = Arc::clone(&self.generation);
        std::thread::spawn(move || {
            std::thread::sleep(IDLE_CLOSE_DELAY);
            if gen_ref.load(Ordering::Acquire) == next_gen {
                let mut g = inner_ref.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(engine) = g.audio.take() {
                    engine.shutdown();
                }
            }
        });

        Ok(played)
    }

    /// Clear cached pack so a refreshed or replaced pack reloads on next audition.
    pub fn invalidate_cache(&self) {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.loaded_pack = None;
        guard.loaded_pack_id = None;
    }

    pub fn shutdown(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(engine) = guard.audio.take() {
            engine.shutdown();
        }
        guard.loaded_pack = None;
        guard.loaded_pack_id = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_zero_is_strictly_silent_without_opening_audio() {
        let player = PreviewPlayer::new();
        let played = player
            .play(Some("KeyA"), "down", "cherry-mx", 0)
            .expect("volume 0 succeeds silently");
        assert!(!played);
        assert!(player.inner.lock().unwrap().audio.is_none());
    }
}
