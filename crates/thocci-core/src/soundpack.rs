//! Mechvibes-format soundpack loader and built-in pack registry.
//!
//! Built-in packs (`cherry-mx` and `banana-split`) are embedded directly in the
//! binary so `thocci` works as a single standalone executable without external
//! `assets/` files or `ffmpeg`. Custom Mechvibes directories on disk are also
//! supported via `--pack <dir>`.

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

pub const BUILTIN_PACKS: &[BuiltinPackMeta] = &[
    BuiltinPackMeta {
        id: "cherry-mx",
        display_name: "CherryMX Black - ABS keycaps",
        folder_name: "cherrymx_brown",
    },
    BuiltinPackMeta {
        id: "banana-split",
        display_name: "Banana Split Lubed by Akira",
        folder_name: "bannasplit",
    },
];

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

#[derive(Debug, Clone)]
pub struct SoundpackInfo {
    pub id: String,
    pub name: String,
    pub key_define_type: String,
    pub includes_numpad: bool,
    pub path: Option<PathBuf>,
}

/// Loaded Mechvibes soundpack ready for fast keycode lookup.
pub struct Soundpack {
    info: SoundpackInfo,
    /// Keyed by Mechvibes (libuiohook) keycode from `config.json`.
    clips: HashMap<u32, SoundClip>,
}

#[derive(Deserialize)]
struct RawConfig {
    name: Option<String>,
    key_define_type: Option<String>,
    includes_numpad: Option<bool>,
    sound: Option<String>,
    defines: Option<HashMap<String, serde_json::Value>>,
}

struct EmbeddedFile {
    name: &'static str,
    bytes: &'static [u8],
}

const CHERRY_MX_CONFIG: &[u8] = include_bytes!("../assets/soundpack/cherrymx_brown/config.json");
const CHERRY_MX_SOUND: &[u8] = include_bytes!("../assets/soundpack/cherrymx_brown/sound.ogg");

const BANANA_CONFIG: &[u8] = include_bytes!("../assets/soundpack/bannasplit/config.json");
const BANANA_FILES: &[EmbeddedFile] = &[
    EmbeddedFile {
        name: "banana-l-1.wav",
        bytes: include_bytes!("../assets/soundpack/bannasplit/banana-l-1.wav"),
    },
    EmbeddedFile {
        name: "banana-l-2.wav",
        bytes: include_bytes!("../assets/soundpack/bannasplit/banana-l-2.wav"),
    },
    EmbeddedFile {
        name: "banana-l-3.wav",
        bytes: include_bytes!("../assets/soundpack/bannasplit/banana-l-3.wav"),
    },
    EmbeddedFile {
        name: "banana-l-4.wav",
        bytes: include_bytes!("../assets/soundpack/bannasplit/banana-l-4.wav"),
    },
    EmbeddedFile {
        name: "banana-l-5.wav",
        bytes: include_bytes!("../assets/soundpack/bannasplit/banana-l-5.wav"),
    },
    EmbeddedFile {
        name: "banana-l-6.wav",
        bytes: include_bytes!("../assets/soundpack/bannasplit/banana-l-6.wav"),
    },
    EmbeddedFile {
        name: "banana-l-7.wav",
        bytes: include_bytes!("../assets/soundpack/bannasplit/banana-l-7.wav"),
    },
];

enum PackSource<'a> {
    Embedded {
        id: &'a str,
        config: &'static [u8],
        files: &'a [EmbeddedFile],
    },
    Directory {
        id: String,
        dir: PathBuf,
    },
}

impl PackSource<'_> {
    fn id(&self) -> &str {
        match self {
            Self::Embedded { id, .. } => id,
            Self::Directory { id, .. } => id,
        }
    }

    fn dir(&self) -> Option<PathBuf> {
        match self {
            Self::Embedded { .. } => None,
            Self::Directory { dir, .. } => Some(dir.clone()),
        }
    }

    fn read_config(&self) -> Result<Vec<u8>> {
        match self {
            Self::Embedded { config, .. } => Ok(config.to_vec()),
            Self::Directory { dir, .. } => {
                let path = dir.join("config.json");
                fs::read(&path)
                    .map_err(|_| Error::new(format!("Missing config.json in {}", dir.display())))
            }
        }
    }

    fn read_audio(&self, filename: &str) -> Result<Vec<u8>> {
        match self {
            Self::Embedded { files, .. } => files
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
    /// Load one of the embedded built-in packs (`cherry-mx`, `banana-split`),
    /// optionally pre-resampling clips to `target_rate` Hz.
    pub fn load_builtin(pack_id: &str, target_rate: Option<u32>) -> Result<Self> {
        let cherry_files = [EmbeddedFile {
            name: "sound.ogg",
            bytes: CHERRY_MX_SOUND,
        }];
        match pack_id {
            "cherry-mx" => Self::load_from_source(
                &PackSource::Embedded {
                    id: "cherry-mx",
                    config: CHERRY_MX_CONFIG,
                    files: &cherry_files,
                },
                target_rate,
            ),
            "banana-split" => Self::load_from_source(
                &PackSource::Embedded {
                    id: "banana-split",
                    config: BANANA_CONFIG,
                    files: BANANA_FILES,
                },
                target_rate,
            ),
            other => Err(Error::new(format!(
                "unknown pack '{other}'. Known: {}",
                known_pack_ids().join(", ")
            ))),
        }
    }

    /// Load a custom Mechvibes soundpack directory from disk.
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
        let raw: RawConfig = serde_json::from_slice(&config_bytes)
            .map_err(|err| Error::new(format!("Invalid config.json: {err}")))?;

        let key_define_type = raw.key_define_type.unwrap_or_else(|| "single".to_owned());
        let name = raw.name.unwrap_or_else(|| source.id().to_owned());
        let info = SoundpackInfo {
            id: source.id().to_owned(),
            name,
            key_define_type: key_define_type.clone(),
            includes_numpad: raw.includes_numpad.unwrap_or(false),
            path: source.dir(),
        };

        let defines = raw.defines.unwrap_or_default();
        let clips = if key_define_type == "single" {
            let sound_name = raw.sound.filter(|name| !name.is_empty()).ok_or_else(|| {
                Error::new("Single-type soundpack requires a 'sound' field in config.json")
            })?;
            load_single_clips(source, &sound_name, &defines, target_rate)?
        } else {
            load_multi_clips(source, &defines, target_rate)?
        };

        if clips.is_empty() {
            return Err(Error::new(format!(
                "No playable key definitions in soundpack '{}'",
                info.name
            )));
        }

        Ok(Self { info, clips })
    }

    pub fn info(&self) -> &SoundpackInfo {
        &self.info
    }

    pub fn key_count(&self) -> usize {
        self.clips.len()
    }

    /// Look up the sound clip for a Linux evdev keycode.
    pub fn clip_for_linux_key(&self, linux_code: u16) -> Option<&SoundClip> {
        if let Some(mv_code) = keymap::mechvibes_code(linux_code) {
            if let Some(clip) = self.clips.get(&mv_code) {
                return Some(clip);
            }
        }
        self.clips.get(&u32::from(linux_code))
    }

    /// Look up a clip by its raw `config.json` keycode (used in tests).
    pub fn clip_for_raw_code(&self, code: u32) -> Option<&SoundClip> {
        self.clips.get(&code)
    }
}

fn load_single_clips(
    source: &PackSource<'_>,
    sound_name: &str,
    defines: &HashMap<String, serde_json::Value>,
    target_rate: Option<u32>,
) -> Result<HashMap<u32, SoundClip>> {
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
        let Some((offset_ms, duration_ms)) = parse_slice_def(value) else {
            continue;
        };
        let start_frame = ((offset_ms as u64 * u64::from(src_rate)) / 1000) as usize;
        let len_frames = ((duration_ms as u64 * u64::from(src_rate)) / 1000) as usize;
        let end_frame = start_frame.saturating_add(len_frames).min(total_frames);
        if start_frame >= end_frame {
            continue;
        }
        let slice = &decoded.stereo[start_frame * 2..end_frame * 2];
        let normalized = resample_stereo(slice, src_rate, dst_rate);
        clips.insert(
            keycode,
            SoundClip {
                samples: Arc::from(normalized),
                sample_rate: dst_rate,
            },
        );
    }
    Ok(clips)
}

fn load_multi_clips(
    source: &PackSource<'_>,
    defines: &HashMap<String, serde_json::Value>,
    target_rate: Option<u32>,
) -> Result<HashMap<u32, SoundClip>> {
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
            clips.insert(keycode, existing.clone());
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
        clips.insert(keycode, clip);
    }
    Ok(clips)
}

fn parse_slice_def(value: &serde_json::Value) -> Option<(u32, u32)> {
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
    fn loads_embedded_cherry_mx() {
        let pack = Soundpack::load_builtin("cherry-mx", Some(48_000)).unwrap();
        assert_eq!(pack.info().id, "cherry-mx");
        assert_eq!(pack.key_count(), 114);
        let a_clip = pack.clip_for_linux_key(30).expect("KEY_A clip");
        assert_eq!(a_clip.sample_rate, 48_000);
        assert!(a_clip.samples.len() > 1000);
        // Extended key (Up arrow = Linux 103 -> Mechvibes 57416) must resolve!
        assert!(pack.clip_for_linux_key(103).is_some());
        assert!(pack.clip_for_linux_key(111).is_some()); // Delete
        assert!(pack.clip_for_linux_key(96).is_some()); // KP Enter
    }

    #[test]
    fn loads_embedded_banana_split() {
        let pack = Soundpack::load_builtin("banana-split", Some(48_000)).unwrap();
        assert_eq!(pack.info().id, "banana-split");
        assert_eq!(pack.key_count(), 118);
        let enter_clip = pack.clip_for_linux_key(28).expect("KEY_ENTER clip");
        assert_eq!(enter_clip.sample_rate, 48_000);
        assert!(enter_clip.samples.len() > 1000);
        assert!(pack.clip_for_linux_key(105).is_some()); // Left arrow
    }

    #[test]
    fn rejects_unknown_builtin() {
        assert!(Soundpack::load_builtin("does-not-exist", None).is_err());
    }
}
