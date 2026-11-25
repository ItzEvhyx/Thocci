//! Thocci core: everything except the user-facing shells.
//!
//! The CLI (`thocci`) and the Tauri app (`thocci-app`) are thin wrappers over
//! this crate:
//!
//! 1. A client (`client`) spawns a headless background daemon (`daemon`).
//! 2. The daemon captures key presses/releases (`input`), maps them into a
//!    soundpack (`soundpack`, `keymap`) and mixes clips to the speakers (`audio`).
//! 3. Clients talk to the daemon over a one-line localhost TCP protocol (`control`).

pub mod audio;
pub mod client;
pub mod control;
pub mod daemon;
mod error;
pub mod input;
pub mod keymap;
pub mod paths;
pub mod process;
pub mod soundpack;

pub use error::{Error, Result};

/// Crate version, shared by the CLI and the app.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
