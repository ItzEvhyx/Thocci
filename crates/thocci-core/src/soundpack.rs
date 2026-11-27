//! v1 & v2 soundpack loader and auto-embedded built-in registry.
//!
//! All soundpacks under `assets/soundpacks/` are embedded directly into the
//! binary at compile time via `build.rs` so `thocci` stays a standalone
//! executable without external `assets/` folders or `ffmpeg`.
//!
//! Supports:
//! - v1 packs (`defines` keyed by numeric libuiohook codes, `single` & `multi`)
//! - v2 packs (`config_version: "2"`, `definitions` keyed by DOM
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
use crate::paths;
use crate::{Error, Result};

pub const DEFAULT_PACK_ID: &str = "cherry-mx";
pub const MAX_IMPORT_WAV_BYTES: u64 = 10 * 1024 * 1024;
pub const MIN_IMPORT_DURATION_MS: f64 = 1.0;
pub const MAX_IMPORT_DURATION_MS: f64 = 10_000.0;

/// Metadata describing a built-in soundpack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct BuiltinPackMeta {
    pub id: &'static str,
    pub display_name: &'static str,
    pub folder_name: &'static str,
    pub source: &'static str,
}

/// Whether a catalog soundpack ships embedded or was imported by the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PackOrigin {
    Builtin,
    Imported,
}

/// Catalog entry shared by the CLI and desktop UI for built-in and imported packs.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PackCatalogEntry {
    pub id: String,
    pub display_name: String,
    pub subtitle: String,
    pub raw_name: String,
    pub origin: PackOrigin,
    pub source: &'static str,
    pub is_default: bool,
}

include!(concat!(env!("OUT_DIR"), "/embedded_packs.rs"));

pub fn is_builtin(pack_id: &str) -> bool {
    BUILTIN_PACKS.iter().any(|pack| pack.id == pack_id)
}

pub fn known_pack_ids() -> Vec<&'static str> {
    BUILTIN_PACKS.iter().map(|pack| pack.id).collect()
}

/// Return `true` if `pack_id` is a safe single-segment identifier for an imported pack.
fn is_safe_pack_id(pack_id: &str) -> bool {
    !pack_id.is_empty()
        && !pack_id.starts_with('.')
        && pack_id.len() <= 64
        && pack_id
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' || ch == '_')
}

/// Locate a user-imported pack directory under `paths::soundpacks_dir()`.
pub fn imported_pack_dir(pack_id: &str) -> Option<PathBuf> {
    if !is_safe_pack_id(pack_id) {
        return None;
    }
    let dir = paths::soundpacks_dir().join(pack_id);
    if dir.join("config.json").is_file() {
        Some(dir)
    } else {
        None
    }
}

pub fn is_imported_pack(pack_id: &str) -> bool {
    imported_pack_dir(pack_id).is_some()
}

pub fn is_known_pack(pack_id: &str) -> bool {
    is_builtin(pack_id) || is_imported_pack(pack_id)
}

fn builtin_catalog_copy(meta: &BuiltinPackMeta) -> (&'static str, &'static str) {
    match meta.id {
        "cherry-mx" => ("Cherry MX Black", "Linear · ABS keycaps"),
        "banana-split" => ("Banana Split", "Lubed linear"),
        "aula-f75-leobog-reaper" => ("AULA F75 Max", "Reaper switches"),
        "clicky-perfect" => ("Clicky Perfect", "Crisp tactile click"),
        "creamy" => ("Creamy", "Soft and muted"),
        "fortnite-keyboard" => ("Fortnite Keyboard", "Clacky gaming"),
        "gravastar-v75" => ("GravaStar V75", "Deep thocky"),
        "hayyoths-keyboard" => ("Hayyoth's Keyboard", "Custom mechanical"),
        "hyperx-alloy-cherry-mx-blue" => ("HyperX Alloy MX Blue", "Clicky"),
        "kailh-saker-mini" => ("Kailh Saker Mini", "Low-profile linear"),
        "keychron-dream-fn-v2" => ("Keychron Dream FN V2", "Smooth custom"),
        "kiiboom-matcha-latte" => ("KiiBOOM Matcha Latte", "Matcha Latte switches"),
        "leobog-hi75-reaper" => ("LEOBOG Hi75", "Reaper switches"),
        "nuphy-halo75-v2-raspberry" => ("NuPhy Halo75 V2", "Raspberry switches"),
        "orange-deluxe" => ("Orange Deluxe", "Press & release"),
        "satisfaction-bubblepop" => ("Satisfaction Bubblepop", "Poppy tactile"),
        "typewriter" => ("Typewriter", "Classic mechanical"),
        "wallhack-k-001" => ("Wallhack K-001", "Light speed linear"),
        "womier-sk75-tmr" => ("Womier SK75 TMR", "Magnetic switches"),
        "wooden-keyboard" => ("Wooden Keyboard", "Dry and hollow"),
        _ => (meta.display_name, "Built-in soundpack"),
    }
}

pub fn builtin_catalog_entry(meta: &BuiltinPackMeta) -> PackCatalogEntry {
    let (display_name, subtitle) = builtin_catalog_copy(meta);
    PackCatalogEntry {
        id: meta.id.to_owned(),
        display_name: display_name.to_owned(),
        subtitle: subtitle.to_owned(),
        raw_name: meta.display_name.to_owned(),
        origin: PackOrigin::Builtin,
        source: meta.source,
        is_default: meta.id == DEFAULT_PACK_ID,
    }
}

/// Discover all valid user-imported soundpacks stored in `paths::soundpacks_dir()`.
pub fn list_imported_packs() -> Vec<PackCatalogEntry> {
    let root = paths::soundpacks_dir();
    let Ok(entries) = fs::read_dir(&root) else {
        return Vec::new();
    };

    let mut packs = Vec::new();
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(id) = path.file_name().and_then(|n| n.to_str()).map(str::to_owned) else {
            continue;
        };
        if !is_safe_pack_id(&id) || is_builtin(&id) {
            continue;
        }
        let config_path = path.join("config.json");
        let Ok(raw_bytes) = fs::read(&config_path) else {
            continue;
        };
        let Ok(config_str) = std::str::from_utf8(&raw_bytes) else {
            continue;
        };
        let Ok(parsed) =
            serde_json::from_str::<RawConfig>(config_str.trim_start_matches('\u{feff}'))
        else {
            continue;
        };
        let name = parsed
            .name
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| id.clone());
        let subtitle = parsed
            .subtitle
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "Imported WAV".to_owned());

        packs.push(PackCatalogEntry {
            id,
            display_name: name.clone(),
            subtitle,
            raw_name: name,
            origin: PackOrigin::Imported,
            source: "Imported WAVs",
            is_default: false,
        });
    }

    packs.sort_by(|a, b| {
        a.display_name
            .to_ascii_lowercase()
            .cmp(&b.display_name.to_ascii_lowercase())
            .then_with(|| a.id.cmp(&b.id))
    });
    packs
}

/// Return all built-in soundpacks followed by valid user-imported packs.
pub fn list_all_packs() -> Vec<PackCatalogEntry> {
    let mut all: Vec<PackCatalogEntry> = BUILTIN_PACKS.iter().map(builtin_catalog_entry).collect();
    all.extend(list_imported_packs());
    all
}

/// Resolve a user-typed pack query across built-in and imported soundpacks.
pub fn resolve_any_pack_query(query: &str) -> Result<PackCatalogEntry> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Err(Error::new(
            "empty soundpack name — run `thocci packs` to list available packs",
        ));
    }

    let all = list_all_packs();
    let lower = trimmed.to_ascii_lowercase();
    let slug = normalize_slug(trimmed);

    // 1. Exact match on id, display name, raw name, or normalized slug.
    if let Some(pack) = all.iter().find(|p| {
        p.id.eq_ignore_ascii_case(trimmed)
            || p.display_name.eq_ignore_ascii_case(trimmed)
            || p.raw_name.eq_ignore_ascii_case(trimmed)
            || (!slug.is_empty()
                && (p.id == slug
                    || normalize_slug(&p.display_name) == slug
                    || normalize_slug(&p.raw_name) == slug))
    }) {
        return Ok(pack.clone());
    }

    // 2. Unique prefix match.
    let prefix_matches: Vec<&PackCatalogEntry> = all
        .iter()
        .filter(|p| {
            (!slug.is_empty() && p.id.starts_with(&slug))
                || p.display_name.to_ascii_lowercase().starts_with(&lower)
                || p.raw_name.to_ascii_lowercase().starts_with(&lower)
        })
        .collect();
    if prefix_matches.len() == 1 {
        return Ok(prefix_matches[0].clone());
    }
    if prefix_matches.len() > 1 {
        let ids: Vec<&str> = prefix_matches.iter().map(|p| p.id.as_str()).collect();
        return Err(Error::new(format!(
            "ambiguous soundpack '{trimmed}' — matches: {}",
            ids.join(", ")
        )));
    }

    // 3. Unique substring match.
    let sub_matches: Vec<&PackCatalogEntry> = all
        .iter()
        .filter(|p| {
            (!slug.is_empty() && p.id.contains(&slug))
                || p.display_name.to_ascii_lowercase().contains(&lower)
                || p.raw_name.to_ascii_lowercase().contains(&lower)
        })
        .collect();
    if sub_matches.len() == 1 {
        return Ok(sub_matches[0].clone());
    }
    if sub_matches.len() > 1 {
        let ids: Vec<&str> = sub_matches.iter().map(|p| p.id.as_str()).collect();
        return Err(Error::new(format!(
            "ambiguous soundpack '{trimmed}' — matches: {}",
            ids.join(", ")
        )));
    }

    Err(Error::new(format!(
        "unknown soundpack '{trimmed}' — run `thocci packs` to see all available packs"
    )))
}

/// Validate a local `.wav` file with the shared decoder and persist it as a reusable
/// v2 generic key-press soundpack in the per-user soundpacks directory.
pub fn import_wav(wav_path: &Path, custom_name: Option<&str>) -> Result<PackCatalogEntry> {
    let meta = fs::metadata(wav_path)
        .map_err(|err| Error::new(format!("Could not read '{}': {err}", wav_path.display())))?;
    if !meta.is_file() {
        return Err(Error::new("Selected path is not a file."));
    }
    if meta.len() == 0 {
        return Err(Error::new("WAV file is empty."));
    }
    if meta.len() > MAX_IMPORT_WAV_BYTES {
        let max_mb = MAX_IMPORT_WAV_BYTES / (1024 * 1024);
        return Err(Error::new(format!(
            "WAV file is too large (maximum {max_mb} MB)."
        )));
    }

    let ext_is_wav = wav_path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("wav"));
    if !ext_is_wav {
        return Err(Error::new("Only .wav files can be imported."));
    }

    let bytes = fs::read(wav_path).map_err(|err| {
        Error::new(format!(
            "Could not read WAV file '{}': {err}",
            wav_path.display()
        ))
    })?;
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(Error::new("File is not a valid RIFF/WAVE audio file."));
    }

    let decoded = decode_audio(bytes.clone(), Some("wav"))
        .map_err(|err| Error::new(format!("Unsupported or unreadable WAV audio: {err}")))?;
    let total_frames = decoded.stereo.len() / 2;
    if total_frames == 0 || decoded.sample_rate == 0 {
        return Err(Error::new("WAV file contains no playable audio frames."));
    }

    let duration_ms = (total_frames as f64 / f64::from(decoded.sample_rate)) * 1000.0;
    if duration_ms < MIN_IMPORT_DURATION_MS {
        return Err(Error::new("WAV clip is too short to play."));
    }
    if duration_ms > MAX_IMPORT_DURATION_MS {
        return Err(Error::new(format!(
            "WAV clip is too long ({:.1} s). Maximum duration is {:.0} s.",
            duration_ms / 1000.0,
            MAX_IMPORT_DURATION_MS / 1000.0
        )));
    }

    let raw_label = custom_name
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            wav_path
                .file_stem()
                .and_then(|s| s.to_str())
                .map(humanize_stem)
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| "Imported WAV".to_owned());

    let display_name: String = raw_label.chars().take(48).collect();
    let base_slug = normalize_slug(&display_name);
    let trimmed_slug = if base_slug.is_empty() {
        "custom-wav".to_owned()
    } else {
        base_slug.chars().take(36).collect()
    };

    let root = paths::soundpacks_dir();
    fs::create_dir_all(&root)?;

    let mut id = format!("imported-{trimmed_slug}");
    let mut suffix = 2usize;
    while is_known_pack(&id) || root.join(&id).exists() {
        id = format!("imported-{trimmed_slug}-{suffix}");
        suffix += 1;
    }

    let staging_name = format!(
        ".staging-{}-{}-{}",
        id,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    let staging_dir = root.join(staging_name);
    fs::create_dir_all(&staging_dir)?;

    let persist_result = (|| -> Result<PackCatalogEntry> {
        let wav_dest = staging_dir.join("sound.wav");
        fs::write(&wav_dest, &bytes)?;

        let duration_rounded = duration_ms.ceil().max(1.0);
        let subtitle = format!("Imported WAV · {} ms", duration_ms.round() as u64);
        let config_json = serde_json::json!({
            "name": display_name,
            "subtitle": subtitle,
            "config_version": "2",
            "definition_method": "single",
            "audio_file": "sound.wav",
            "definitions": {
                "Generic": {
                    "audio_file": "sound.wav",
                    "timing": [[0.0, duration_rounded]]
                }
            }
        });
        let config_bytes = serde_json::to_vec_pretty(&config_json)
            .map_err(|err| Error::new(format!("Could not serialize pack config: {err}")))?;
        fs::write(staging_dir.join("config.json"), config_bytes)?;

        // Verify the complete persisted format decodes and resolves keys cleanly before committing.
        let verified = Soundpack::load_from_dir(&staging_dir, Some(&id), None)?;
        if verified.clip_for_linux_key(keymap::KEY_A).is_none() {
            return Err(Error::new(
                "Imported WAV pack failed verification (missing fallback clip).",
            ));
        }

        let final_dir = root.join(&id);
        fs::rename(&staging_dir, &final_dir)?;

        Ok(PackCatalogEntry {
            id: id.clone(),
            display_name: display_name.clone(),
            subtitle,
            raw_name: display_name,
            origin: PackOrigin::Imported,
            source: "Imported WAVs",
            is_default: false,
        })
    })();

    if persist_result.is_err() {
        let _ = fs::remove_dir_all(&staging_dir);
    }

    persist_result
}

fn humanize_stem(stem: &str) -> String {
    let cleaned: String = stem
        .chars()
        .map(|ch| match ch {
            '_' | '-' => ' ',
            other => other,
        })
        .collect();
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Resolve a user-typed pack query (id, display name, or unique partial match)
/// to a built-in soundpack.
///
/// Examples: `"creamy"`, `"Wooden Keyboard"`, `"wooden-keyboard"`, `"matcha"`.
pub fn resolve_pack_query(query: &str) -> Result<&'static BuiltinPackMeta> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Err(Error::new(
            "empty soundpack name — run `thocci packs` to list available packs",
        ));
    }

    let lower = trimmed.to_ascii_lowercase();
    let slug = normalize_slug(trimmed);

    // 1. Exact match on id, display name, or normalized slug.
    if let Some(pack) = BUILTIN_PACKS.iter().find(|p| {
        p.id.eq_ignore_ascii_case(trimmed)
            || p.display_name.eq_ignore_ascii_case(trimmed)
            || (!slug.is_empty() && (p.id == slug || normalize_slug(p.display_name) == slug))
    }) {
        return Ok(pack);
    }

    // 2. Unique prefix match on id or normalized display name.
    let prefix_matches: Vec<&BuiltinPackMeta> = BUILTIN_PACKS
        .iter()
        .filter(|p| {
            p.id.starts_with(&slug) || p.display_name.to_ascii_lowercase().starts_with(&lower)
        })
        .collect();
    if prefix_matches.len() == 1 {
        return Ok(prefix_matches[0]);
    }
    if prefix_matches.len() > 1 {
        return Err(ambiguous_error(trimmed, &prefix_matches));
    }

    // 3. Unique substring match on id or display name.
    let sub_matches: Vec<&BuiltinPackMeta> = BUILTIN_PACKS
        .iter()
        .filter(|p| {
            (!slug.is_empty() && p.id.contains(&slug))
                || p.display_name.to_ascii_lowercase().contains(&lower)
        })
        .collect();
    if sub_matches.len() == 1 {
        return Ok(sub_matches[0]);
    }
    if sub_matches.len() > 1 {
        return Err(ambiguous_error(trimmed, &sub_matches));
    }

    Err(Error::new(format!(
        "unknown soundpack '{trimmed}' — run `thocci packs` to see all available packs"
    )))
}

fn normalize_slug(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut prev_dash = true;
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    if out.ends_with('-') {
        out.pop();
    }
    out
}

fn ambiguous_error(query: &str, matches: &[&BuiltinPackMeta]) -> Error {
    let ids: Vec<&str> = matches.iter().map(|p| p.id).collect();
    Error::new(format!(
        "ambiguous soundpack '{query}' — matches: {}",
        ids.join(", ")
    ))
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

/// Loaded v1 or v2 soundpack ready for fast keycode lookup.
pub struct Soundpack {
    info: SoundpackInfo,
    /// Direct Linux keycode map (populated by v2 packs).
    linux_keys: HashMap<u16, KeySound>,
    /// Numeric libuiohook keycode map (populated by v1 packs).
    uiohook_keys: HashMap<u32, KeySound>,
    /// Optional fallback sound for v2 packs defining `"Generic"` or `"key"`.
    fallback: Option<KeySound>,
}

#[derive(Deserialize)]
struct RawConfig {
    name: Option<String>,
    subtitle: Option<String>,
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

    /// Load either a built-in pack or a user-imported pack by its ID.
    pub fn load_by_id(pack_id: &str, target_rate: Option<u32>) -> Result<Self> {
        if is_builtin(pack_id) {
            return Self::load_builtin(pack_id, target_rate);
        }
        if let Some(dir) = imported_pack_dir(pack_id) {
            return Self::load_from_dir(&dir, Some(pack_id), target_rate);
        }
        Err(Error::new(format!(
            "unknown pack '{pack_id}'. Known built-ins: {}",
            known_pack_ids().join(", ")
        )))
    }

    /// Load a custom v1 or v2 soundpack directory from disk.
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
                uiohook_keys: HashMap::new(),
                fallback,
            });
        }

        let defines = raw.defines.unwrap_or_default();
        let uiohook_keys = if key_define_type == "single" {
            let sound_name = raw.sound.filter(|name| !name.is_empty()).ok_or_else(|| {
                Error::new("Single-type soundpack requires a 'sound' field in config.json")
            })?;
            load_v1_single(source, &sound_name, &defines, target_rate)?
        } else {
            load_v1_multi(source, &defines, target_rate)?
        };

        if uiohook_keys.is_empty() {
            return Err(Error::new(format!(
                "No playable key definitions in soundpack '{}'",
                info.name
            )));
        }

        Ok(Self {
            info,
            linux_keys: HashMap::new(),
            uiohook_keys,
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
            self.uiohook_keys.len()
        }
    }

    /// Look up press/release sounds for a Linux evdev keycode.
    pub fn sound_for_linux_key(&self, linux_code: u16) -> Option<&KeySound> {
        if let Some(sound) = self.linux_keys.get(&linux_code) {
            return Some(sound);
        }
        if let Some(uh_code) = keymap::uiohook_code(linux_code) {
            if let Some(sound) = self.uiohook_keys.get(&uh_code) {
                return Some(sound);
            }
        }
        if let Some(sound) = self.uiohook_keys.get(&u32::from(linux_code)) {
            return Some(sound);
        }
        self.fallback.as_ref()
    }

    /// Convenience accessor for a key's press clip.
    pub fn clip_for_linux_key(&self, linux_code: u16) -> Option<&SoundClip> {
        self.sound_for_linux_key(linux_code).map(|s| &s.down)
    }

    /// Look up press/release sounds for a DOM `KeyboardEvent.code`, falling back
    /// to a representative key sample if the pack has no specific entry.
    pub fn sound_for_dom_code(&self, code: &str) -> Option<&KeySound> {
        if let Some(linux_code) = keymap::dom_code_to_linux(code) {
            if let Some(sound) = self.sound_for_linux_key(linux_code) {
                return Some(sound);
            }
        }
        self.representative_sound()
    }

    /// Return a representative key sound for non-key UI controls (e.g. Quick Actions icons).
    pub fn representative_sound(&self) -> Option<&KeySound> {
        for candidate in [keymap::KEY_A, 57, 28, 31, 32] {
            if let Some(sound) = self.sound_for_linux_key(candidate) {
                return Some(sound);
            }
        }
        self.fallback
            .as_ref()
            .or_else(|| self.linux_keys.values().next())
            .or_else(|| self.uiohook_keys.values().next())
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
        assert_eq!(BUILTIN_PACKS.len(), 41);
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
    fn catalog_uses_source_folders_without_changing_pack_ids() {
        for (id, source) in [
            (DEFAULT_PACK_ID, "Original library"),
            ("cherrymx-blue-pbt", "Mechvibes"),
            ("alpaca", "kbsim"),
        ] {
            let entry = resolve_any_pack_query(id).unwrap();
            assert_eq!(entry.id, id);
            assert_eq!(entry.source, source);
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
    fn kbsim_packs_keep_key_rows_and_release_samples() {
        let pack = Soundpack::load_builtin("alpaca", Some(48_000)).unwrap();
        let home = pack.sound_for_dom_code("KeyA").unwrap();
        let top = pack.sound_for_dom_code("KeyQ").unwrap();
        let space = pack.sound_for_dom_code("Space").unwrap();
        assert_ne!(home.down.samples.as_ref(), top.down.samples.as_ref());
        assert_ne!(home.down.samples.as_ref(), space.down.samples.as_ref());
        assert_ne!(
            home.up.as_ref().unwrap().samples.as_ref(),
            space.up.as_ref().unwrap().samples.as_ref()
        );
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

    #[test]
    fn resolves_pack_queries_by_id_name_and_substring() {
        assert_eq!(resolve_pack_query("creamy").unwrap().id, "creamy");
        assert_eq!(
            resolve_pack_query("Wooden Keyboard").unwrap().id,
            "wooden-keyboard"
        );
        assert_eq!(
            resolve_pack_query("wooden-keyboard").unwrap().id,
            "wooden-keyboard"
        );
        assert_eq!(
            resolve_pack_query("matcha").unwrap().id,
            "kiiboom-matcha-latte"
        );
        assert_eq!(resolve_pack_query("orange").unwrap().id, "orange-deluxe");
        // "reaper" matches both aula-f75-leobog-reaper and leobog-hi75-reaper.
        assert!(resolve_pack_query("reaper").is_err());
        assert!(resolve_pack_query("totally-unknown-pack").is_err());
    }

    fn make_pcm_wav(sample_rate: u32, num_frames: usize) -> Vec<u8> {
        let num_channels: u16 = 1;
        let bits_per_sample: u16 = 16;
        let byte_rate = sample_rate * u32::from(num_channels) * u32::from(bits_per_sample / 8);
        let block_align = num_channels * (bits_per_sample / 8);
        let data_len = (num_frames * usize::from(block_align)) as u32;
        let riff_len = 36 + data_len;

        let mut buf = Vec::with_capacity((44 + data_len) as usize);
        buf.extend_from_slice(b"RIFF");
        buf.extend_from_slice(&riff_len.to_le_bytes());
        buf.extend_from_slice(b"WAVE");
        buf.extend_from_slice(b"fmt ");
        buf.extend_from_slice(&16u32.to_le_bytes());
        buf.extend_from_slice(&1u16.to_le_bytes()); // PCM
        buf.extend_from_slice(&num_channels.to_le_bytes());
        buf.extend_from_slice(&sample_rate.to_le_bytes());
        buf.extend_from_slice(&byte_rate.to_le_bytes());
        buf.extend_from_slice(&block_align.to_le_bytes());
        buf.extend_from_slice(&bits_per_sample.to_le_bytes());
        buf.extend_from_slice(b"data");
        buf.extend_from_slice(&data_len.to_le_bytes());
        for i in 0..num_frames {
            let sample = ((i as f32 * 0.15).sin() * 12_000.0) as i16;
            buf.extend_from_slice(&sample.to_le_bytes());
        }
        buf
    }

    #[test]
    fn imports_valid_wav_persists_and_resolves() {
        let _guard = crate::paths::test_support::isolated_state();
        let tmp_wav = crate::paths::state_dir().join("custom_thock.wav");
        std::fs::write(&tmp_wav, make_pcm_wav(44_100, 2_205)).unwrap();

        let entry = import_wav(&tmp_wav, None).expect("valid WAV imports cleanly");
        assert_eq!(entry.origin, PackOrigin::Imported);
        assert_eq!(entry.source, "Imported WAVs");
        assert!(entry.id.starts_with("imported-custom-thock"));
        assert!(is_known_pack(&entry.id));

        // Persists across listing, query resolution, saved pack, and loading.
        assert!(list_all_packs().iter().any(|p| p.id == entry.id));
        assert_eq!(resolve_any_pack_query(&entry.id).unwrap().id, entry.id);

        let loaded = Soundpack::load_by_id(&entry.id, Some(48_000)).unwrap();
        let clip = loaded
            .sound_for_dom_code("KeyA")
            .expect("generic fallback resolves KeyA");
        assert_eq!(clip.down.sample_rate, 48_000);
        assert!(!clip.down.samples.is_empty());

        crate::paths::write_saved_pack(&entry.id).unwrap();
        assert_eq!(crate::paths::read_saved_pack(), entry.id);

        // Clean up imported pack so other tests see a clean state.
        let _ = std::fs::remove_dir_all(crate::paths::soundpacks_dir());
        let _ = std::fs::remove_file(&tmp_wav);
        crate::paths::write_saved_pack(DEFAULT_PACK_ID).unwrap();
    }

    #[test]
    fn rejects_invalid_wav_imports_and_preserves_working_pack() {
        let _guard = crate::paths::test_support::isolated_state();
        crate::paths::write_saved_pack("creamy").unwrap();

        // Empty file
        let empty = crate::paths::state_dir().join("empty.wav");
        std::fs::write(&empty, b"").unwrap();
        assert!(import_wav(&empty, None).is_err());

        // Non-WAV extension
        let txt = crate::paths::state_dir().join("notes.txt");
        std::fs::write(&txt, b"hello").unwrap();
        assert!(import_wav(&txt, None).is_err());

        // Corrupt bytes with .wav extension
        let bad = crate::paths::state_dir().join("corrupt.wav");
        std::fs::write(&bad, b"not a real wav file at all").unwrap();
        assert!(import_wav(&bad, None).is_err());

        // Over-long WAV (> 10s)
        let too_long = crate::paths::state_dir().join("too_long.wav");
        std::fs::write(&too_long, make_pcm_wav(8_000, 90_000)).unwrap();
        assert!(import_wav(&too_long, None).is_err());

        // Failed imports leave saved pack untouched and leave no broken packs behind.
        assert_eq!(crate::paths::read_saved_pack(), "creamy");
        assert!(list_imported_packs().is_empty());

        for f in [&empty, &txt, &bad, &too_long] {
            let _ = std::fs::remove_file(f);
        }
    }
}
