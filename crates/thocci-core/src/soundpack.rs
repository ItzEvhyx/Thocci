//! Mechvibes v1 & v2 soundpack loader and auto-embedded built-in registry.
//!
//! All soundpacks under `assets/soundpacks/` are embedded directly into the
//! binary at compile time via `build.rs` so `thocci` stays a standalone
//! executable without external `assets/` folders or `ffmpeg`.
//!
//! Supports:
//! - Mechvibes v1 (`defines` keyed by numeric libuiohook codes, `single` & `multi`)
//! - Mechvibes v2 (`config_version: "2"`, `definitions` keyed by DOM
//!   `KeyboardEvent.code` names, `[start_ms, end_ms]` press & optional release
//!   timings, `single` & `multi`, plus `"Generic"`/`"key"` fallbacks)
//! - Audio formats decoded in pure Rust via Symphonia: `.ogg`, `.wav`, `.mp3`.

use std::collections::HashMap;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Deserialize;
use symphonia::core::audio::sample::Sample;
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::{MediaSource, MediaSourceStream};
use symphonia::core::meta::MetadataOptions;

use crate::keymap;
use crate::{Error, Result};

pub const DEFAULT_PACK_ID: &str = "cherry-mx";

/// Metadata describing a built-in soundpack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct BuiltinPackMeta {
    pub id: &'static str,
    pub display_name: &'static str,
    pub folder_name: &'static str,
}

include!(concat!(env!("OUT_DIR"), "/embedded_packs.rs"));

pub fn is_builtin(pack_id: &str) -> bool {
    BUILTIN_PACKS.iter().any(|pack| pack.id == pack_id)
}

pub fn known_pack_ids() -> Vec<&'static str> {
    BUILTIN_PACKS.iter().map(|pack| pack.id).collect()
}

/// Decoded interleaved stereo PCM clip ready for mixing.
#[derive(Debug, Clone)]
pub struct SoundClip {
    /// Interleaved stereo `f32` samples (`[L0, R0, L1, R1, ...]`).
    pub samples: Arc<[f32]>,
    pub sample_rate: u32,
}

/// Press and optional release clip for one key.
#[derive(Debug, Clone)]
pub struct KeySound {
    pub down: SoundClip,
    pub up: Option<SoundClip>,
}

#[derive(Debug, Clone)]
pub struct SoundpackInfo {
    pub id: String,
    pub name: String,
    pub key_define_type: String,
    pub includes_numpad: bool,
    pub path: Option<PathBuf>,
}

/// Loaded Mechvibes v1 or v2 soundpack ready for fast keycode lookup.
pub struct Soundpack {
    info: SoundpackInfo,
    /// Direct Linux keycode map (populated by Mechvibes v2 packs).
    linux_keys: HashMap<u16, KeySound>,
    /// Numeric Mechvibes (libuiohook) keycode map (populated by v1 packs).
    mechvibes_keys: HashMap<u32, KeySound>,
    /// Optional fallback sound for v2 packs defining `"Generic"` or `"key"`.
    fallback: Option<KeySound>,
}

#[derive(Deserialize)]
struct RawConfig {
    name: Option<String>,
    config_version: Option<serde_json::Value>,
    // v1 fields
    key_define_type: Option<String>,
    includes_numpad: Option<bool>,
    sound: Option<String>,
    defines: Option<HashMap<String, serde_json::Value>>,
    // v2 fields
    definition_method: Option<String>,
    audio_file: Option<String>,
    definitions: Option<HashMap<String, RawV2Def>>,
}

#[derive(Deserialize)]
struct RawV2Def {
    audio_file: Option<String>,
    timing: Option<Vec<Vec<f64>>>,
}

enum PackSource<'a> {
    Embedded(&'a EmbeddedPack),
    Directory { id: String, dir: PathBuf },
}

impl PackSource<'_> {
    fn id(&self) -> &str {
        match self {
            Self::Embedded(pack) => pack.meta.id,
            Self::Directory { id, .. } => id,
        }
    }

    fn dir(&self) -> Option<PathBuf> {
        match self {
            Self::Embedded(_) => None,
            Self::Directory { dir, .. } => Some(dir.clone()),
        }
    }

    fn read_config(&self) -> Result<Vec<u8>> {
        match self {
            Self::Embedded(pack) => Ok(pack.config.to_vec()),
            Self::Directory { dir, .. } => {
                let path = dir.join("config.json");
                fs::read(&path)
                    .map_err(|_| Error::new(format!("Missing config.json in {}", dir.display())))
            }
        }
    }

    fn read_audio(&self, filename: &str) -> Result<Vec<u8>> {
        match self {
            Self::Embedded(pack) => pack
                .files
                .iter()
                .find(|entry| entry.name == filename)
                .map(|entry| entry.bytes.to_vec())
                .ok_or_else(|| Error::new(format!("Embedded sound file not found: {filename}"))),
            Self::Directory { dir, .. } => {
                let path = dir.join(filename);
                fs::read(&path)
                    .map_err(|_| Error::new(format!("Sound file not found: {}", path.display())))
            }
        }
    }
}

impl Soundpack {
    /// Load any embedded built-in pack from `assets/soundpacks/`.
    pub fn load_builtin(pack_id: &str, target_rate: Option<u32>) -> Result<Self> {
        let embedded = EMBEDDED_PACKS
            .iter()
            .find(|pack| pack.meta.id == pack_id)
            .ok_or_else(|| {
                Error::new(format!(
                    "unknown pack '{pack_id}'. Known: {}",
                    known_pack_ids().join(", ")
                ))
            })?;
        Self::load_from_source(&PackSource::Embedded(embedded), target_rate)
    }

    /// Load a custom Mechvibes v1 or v2 soundpack directory from disk.
    pub fn load_from_dir(
        dir: &Path,
        pack_id: Option<&str>,
        target_rate: Option<u32>,
    ) -> Result<Self> {
        let resolved = fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
        let id = pack_id
            .map(str::to_owned)
            .or_else(|| {
                resolved
                    .file_name()
                    .and_then(|name| name.to_str())
                    .and_then(|folder| {
                        BUILTIN_PACKS
                            .iter()
                            .find(|pack| pack.folder_name == folder)
                            .map(|pack| pack.id.to_owned())
                    })
            })
            .unwrap_or_else(|| "custom".to_owned());

        Self::load_from_source(&PackSource::Directory { id, dir: resolved }, target_rate)
    }

    fn load_from_source(source: &PackSource<'_>, target_rate: Option<u32>) -> Result<Self> {
        let config_bytes = source.read_config()?;
        let config_str = std::str::from_utf8(&config_bytes)
            .map_err(|err| Error::new(format!("Invalid UTF-8 in config.json: {err}")))?
            .trim_start_matches('\u{feff}');
        let raw: RawConfig = serde_json::from_str(config_str)
            .map_err(|err| Error::new(format!("Invalid config.json: {err}")))?;

        let is_v2 = raw.definitions.is_some()
            || raw
                .config_version
                .as_ref()
                .is_some_and(|v| v.as_str() == Some("2") || v.as_u64() == Some(2));

        let key_define_type = if is_v2 {
            raw.definition_method
                .clone()
                .unwrap_or_else(|| "single".to_owned())
        } else {
            raw.key_define_type
                .clone()
                .unwrap_or_else(|| "single".to_owned())
        };

        let info = SoundpackInfo {
            id: source.id().to_owned(),
            name: raw.name.unwrap_or_else(|| source.id().to_owned()),
            key_define_type: key_define_type.clone(),
            includes_numpad: raw.includes_numpad.unwrap_or(false),
            path: source.dir(),
        };

        if is_v2 {
            let defs = raw.definitions.unwrap_or_default();
            let default_audio = raw.audio_file.as_deref();
            let (linux_keys, fallback) = load_v2_clips(source, default_audio, &defs, target_rate)?;
            if linux_keys.is_empty() && fallback.is_none() {
                return Err(Error::new(format!(
                    "No playable key definitions in soundpack '{}'",
                    info.name
                )));
            }
            return Ok(Self {
                info,
                linux_keys,
                mechvibes_keys: HashMap::new(),
                fallback,
            });
        }

        let defines = raw.defines.unwrap_or_default();
        let mechvibes_keys = if key_define_type == "single" {
            let sound_name = raw.sound.filter(|name| !name.is_empty()).ok_or_else(|| {
                Error::new("Single-type soundpack requires a 'sound' field in config.json")
            })?;
            load_v1_single(source, &sound_name, &defines, target_rate)?
        } else {
            load_v1_multi(source, &defines, target_rate)?
        };

        if mechvibes_keys.is_empty() {
            return Err(Error::new(format!(
                "No playable key definitions in soundpack '{}'",
                info.name
            )));
        }

        Ok(Self {
            info,
            linux_keys: HashMap::new(),
            mechvibes_keys,
            fallback: None,
        })
    }

    pub fn info(&self) -> &SoundpackInfo {
        &self.info
    }

    pub fn key_count(&self) -> usize {
        if !self.linux_keys.is_empty() {
            self.linux_keys.len()
        } else {
            self.mechvibes_keys.len()
        }
    }

    /// Look up press/release sounds for a Linux evdev keycode.
    pub fn sound_for_linux_key(&self, linux_code: u16) -> Option<&KeySound> {
        if let Some(sound) = self.linux_keys.get(&linux_code) {
            return Some(sound);
        }
        if let Some(mv_code) = keymap::mechvibes_code(linux_code) {
            if let Some(sound) = self.mechvibes_keys.get(&mv_code) {
                return Some(sound);
            }
        }
        if let Some(sound) = self.mechvibes_keys.get(&u32::from(linux_code)) {
            return Some(sound);
        }
        self.fallback.as_ref()
    }

    /// Convenience accessor for a key's press clip.
    pub fn clip_for_linux_key(&self, linux_code: u16) -> Option<&SoundClip> {
        self.sound_for_linux_key(linux_code).map(|s| &s.down)
    }
}

fn load_v2_clips(
    source: &PackSource<'_>,
    default_audio: Option<&str>,
    definitions: &HashMap<String, RawV2Def>,
    target_rate: Option<u32>,
) -> Result<(HashMap<u16, KeySound>, Option<KeySound>)> {
    let mut decoded_files: HashMap<String, DecodedAudio> = HashMap::new();
    let mut linux_keys = HashMap::new();
    let mut fallback = None;

    for (key_name, def) in definitions {
        let Some(audio_name) = def.audio_file.as_deref().or(default_audio) else {
            continue;
        };
        if !decoded_files.contains_key(audio_name) {
            let Ok(bytes) = source.read_audio(audio_name) else {
                continue;
            };
            let decoded = decode_audio(bytes, extension_hint(audio_name))?;
            decoded_files.insert(audio_name.to_owned(), decoded);
        }
        let decoded = &decoded_files[audio_name];
        let Some(sound) = build_v2_key_sound(decoded, def.timing.as_deref(), target_rate) else {
            continue;
        };

        if matches!(key_name.as_str(), "Generic" | "key") {
            fallback = Some(sound);
        } else if let Some(linux_code) = keymap::dom_code_to_linux(key_name) {
            linux_keys.insert(linux_code, sound);
        }
    }

    Ok((linux_keys, fallback))
}

fn build_v2_key_sound(
    decoded: &DecodedAudio,
    timings: Option<&[Vec<f64>]>,
    target_rate: Option<u32>,
) -> Option<KeySound> {
    let timings = timings?;
    let first = timings.first()?;
    let down = slice_v2_range(decoded, first, target_rate)?;
    let up = timings
        .get(1)
        .and_then(|pair| slice_v2_range(decoded, pair, target_rate));
    Some(KeySound { down, up })
}

fn slice_v2_range(
    decoded: &DecodedAudio,
    pair: &[f64],
    target_rate: Option<u32>,
) -> Option<SoundClip> {
    if pair.len() < 2 {
        return None;
    }
    let start_ms = pair[0].max(0.0);
    let end_ms = pair[1];
    if end_ms <= start_ms {
        return None;
    }

    let total_frames = decoded.stereo.len() / 2;
    let src_rate = decoded.sample_rate;
    let start_frame = ((start_ms * f64::from(src_rate)) / 1000.0).round() as usize;
    let end_frame = (((end_ms * f64::from(src_rate)) / 1000.0).round() as usize).min(total_frames);
    if start_frame >= end_frame {
        return None;
    }

    let dst_rate = target_rate.unwrap_or(src_rate);
    let slice = &decoded.stereo[start_frame * 2..end_frame * 2];
    let normalized = resample_stereo(slice, src_rate, dst_rate);
    Some(SoundClip {
        samples: Arc::from(normalized),
        sample_rate: dst_rate,
    })
}

fn load_v1_single(
    source: &PackSource<'_>,
    sound_name: &str,
    defines: &HashMap<String, serde_json::Value>,
    target_rate: Option<u32>,
) -> Result<HashMap<u32, KeySound>> {
    let bytes = source.read_audio(sound_name)?;
    let decoded = decode_audio(bytes, extension_hint(sound_name))?;
    let total_frames = decoded.stereo.len() / 2;
    let src_rate = decoded.sample_rate;
    let dst_rate = target_rate.unwrap_or(src_rate);

    let mut clips = HashMap::new();
    for (key, value) in defines {
        let Ok(keycode) = key.parse::<u32>() else {
            continue;
        };
        let Some((offset_ms, duration_ms)) = parse_v1_slice(value) else {
            continue;
        };
        let start_frame = ((u64::from(offset_ms) * u64::from(src_rate)) / 1000) as usize;
        let len_frames = ((u64::from(duration_ms) * u64::from(src_rate)) / 1000) as usize;
        let end_frame = start_frame.saturating_add(len_frames).min(total_frames);
        if start_frame >= end_frame {
            continue;
        }
        let slice = &decoded.stereo[start_frame * 2..end_frame * 2];
        let normalized = resample_stereo(slice, src_rate, dst_rate);
        clips.insert(
            keycode,
            KeySound {
                down: SoundClip {
                    samples: Arc::from(normalized),
                    sample_rate: dst_rate,
                },
                up: None,
            },
        );
    }
    Ok(clips)
}

fn load_v1_multi(
    source: &PackSource<'_>,
    defines: &HashMap<String, serde_json::Value>,
    target_rate: Option<u32>,
) -> Result<HashMap<u32, KeySound>> {
    let mut cache: HashMap<String, SoundClip> = HashMap::new();
    let mut clips = HashMap::new();

    for (key, value) in defines {
        let Ok(keycode) = key.parse::<u32>() else {
            continue;
        };
        let Some(filename) = value.as_str().filter(|name| !name.is_empty()) else {
            continue;
        };
        if let Some(existing) = cache.get(filename) {
            clips.insert(
                keycode,
                KeySound {
                    down: existing.clone(),
                    up: None,
                },
            );
            continue;
        }
        let Ok(bytes) = source.read_audio(filename) else {
            continue;
        };
        let decoded = decode_audio(bytes, extension_hint(filename))?;
        let dst_rate = target_rate.unwrap_or(decoded.sample_rate);
        let normalized = resample_stereo(&decoded.stereo, decoded.sample_rate, dst_rate);
        let clip = SoundClip {
            samples: Arc::from(normalized),
            sample_rate: dst_rate,
        };
        cache.insert(filename.to_owned(), clip.clone());
        clips.insert(
            keycode,
            KeySound {
                down: clip,
                up: None,
            },
        );
    }
    Ok(clips)
}

fn parse_v1_slice(value: &serde_json::Value) -> Option<(u32, u32)> {
    let arr = value.as_array()?;
    if arr.len() < 2 {
        return None;
    }
    let offset = arr[0].as_i64()?;
    let duration = arr[1].as_i64()?;
    if offset < 0 || duration <= 0 {
        return None;
    }
    Some((offset as u32, duration as u32))
}

fn extension_hint(filename: &str) -> Option<&str> {
    Path::new(filename).extension().and_then(|ext| ext.to_str())
}

struct DecodedAudio {
    /// Interleaved stereo `f32` samples.
    stereo: Vec<f32>,
    sample_rate: u32,
}

fn decode_audio(bytes: Vec<u8>, ext: Option<&str>) -> Result<DecodedAudio> {
    let source: Box<dyn MediaSource> = Box::new(Cursor::new(bytes));
    let mss = MediaSourceStream::new(source, Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = ext {
        hint.with_extension(ext);
    }

    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            mss,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|err| Error::new(format!("Audio probe failed: {err}")))?;

    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(|| Error::new("No audio track found"))?;
    let track_id = track.id;
    let audio_params = track
        .codec_params
        .as_ref()
        .and_then(|params| params.audio())
        .ok_or_else(|| Error::new("Missing audio codec parameters"))?;
    let mut sample_rate = audio_params.sample_rate.unwrap_or(44_100);

    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(audio_params, &AudioDecoderOptions::default())
        .map_err(|err| Error::new(format!("Audio decoder init failed: {err}")))?;

    let mut stereo = Vec::new();
    let mut scratch: Vec<f32> = Vec::new();

    while let Some(packet) = format
        .next_packet()
        .map_err(|err| Error::new(format!("Audio demux failed: {err}")))?
    {
        if packet.track_id != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(audio_buf) => {
                let spec = audio_buf.spec();
                sample_rate = spec.rate();
                let channels = spec.channels().count().max(1);
                let count = audio_buf.samples_interleaved();
                scratch.resize(count, f32::MID);
                audio_buf.copy_to_slice_interleaved(&mut scratch);
                append_as_stereo(&scratch, channels, &mut stereo);
            }
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(err) => return Err(Error::new(format!("Audio decode failed: {err}"))),
        }
    }

    Ok(DecodedAudio {
        stereo,
        sample_rate,
    })
}

fn append_as_stereo(interleaved: &[f32], channels: usize, out: &mut Vec<f32>) {
    match channels {
        1 => {
            out.reserve(interleaved.len() * 2);
            for &sample in interleaved {
                out.push(sample);
                out.push(sample);
            }
        }
        2 => out.extend_from_slice(interleaved),
        n => {
            out.reserve((interleaved.len() / n) * 2);
            for frame in interleaved.chunks_exact(n) {
                out.push(frame[0]);
                out.push(frame[1]);
            }
        }
    }
}

/// Linearly resample interleaved stereo audio from `src_rate` to `dst_rate`.
pub(crate) fn resample_stereo(stereo: &[f32], src_rate: u32, dst_rate: u32) -> Vec<f32> {
    let src_frames = stereo.len() / 2;
    if src_rate == dst_rate || src_frames <= 1 || src_rate == 0 || dst_rate == 0 {
        return stereo.to_vec();
    }

    let dst_frames =
        ((src_frames as f64 * f64::from(dst_rate) / f64::from(src_rate)).round() as usize).max(1);
    let mut out = Vec::with_capacity(dst_frames * 2);
    let ratio = (src_frames - 1) as f64 / dst_frames.max(1) as f64;

    for frame in 0..dst_frames {
        let pos = frame as f64 * ratio;
        let idx = (pos.floor() as usize).min(src_frames - 1);
        let next = (idx + 1).min(src_frames - 1);
        let frac = (pos - idx as f64) as f32;

        let l0 = stereo[idx * 2];
        let r0 = stereo[idx * 2 + 1];
        let l1 = stereo[next * 2];
        let r1 = stereo[next * 2 + 1];

        out.push(l0 + (l1 - l0) * frac);
        out.push(r0 + (r1 - r0) * frac);
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_all_embedded_soundpacks() {
        assert_eq!(BUILTIN_PACKS.len(), 20);
        for meta in BUILTIN_PACKS {
            let pack = Soundpack::load_builtin(meta.id, Some(48_000))
                .unwrap_or_else(|err| panic!("failed to load '{}': {err}", meta.id));
            assert!(pack.key_count() > 0, "pack '{}' has 0 keys", meta.id);
            let a_clip = pack
                .clip_for_linux_key(30)
                .unwrap_or_else(|| panic!("pack '{}' missing KEY_A", meta.id));
            assert_eq!(a_clip.sample_rate, 48_000);
            assert!(
                a_clip.samples.len() > 200,
                "pack '{}' KEY_A too short",
                meta.id
            );
            assert!(
                pack.clip_for_linux_key(57).is_some(),
                "pack '{}' missing Space",
                meta.id
            );
        }
    }

    #[test]
    fn loads_press_and_release_sounds_for_v2_pack() {
        let pack = Soundpack::load_builtin("orange-deluxe", Some(48_000)).unwrap();
        let sound = pack.sound_for_linux_key(30).expect("KEY_A sound");
        assert!(sound.down.samples.len() > 200);
        assert!(sound.up.as_ref().is_some_and(|u| u.samples.len() > 200));
    }

    #[test]
    fn v1_extended_keys_resolve() {
        let pack = Soundpack::load_builtin("cherry-mx", Some(48_000)).unwrap();
        assert!(pack.clip_for_linux_key(103).is_some()); // Up
        assert!(pack.clip_for_linux_key(111).is_some()); // Delete
        assert!(pack.clip_for_linux_key(96).is_some()); // KP Enter
    }

    #[test]
    fn rejects_unknown_builtin() {
        assert!(Soundpack::load_builtin("does-not-exist", None).is_err());
    }
}
