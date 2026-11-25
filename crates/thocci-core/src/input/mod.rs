//! Cross-platform global keyboard capture.
//!
//! Each platform backend invokes `on_key_down(linux_keycode)` once per physical
//! key press (auto-repeat is ignored on every OS) and skips ignored keys
//! (`CapsLock`, `NumLock`, `ScrollLock`, `Super`/`Win`/`Cmd`).

use std::sync::Arc;

use crate::Result;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

pub type KeyDownCallback = Arc<dyn Fn(u16) + Send + Sync + 'static>;

pub trait KeyListener: Send {
    fn start(&mut self, on_key_down: KeyDownCallback) -> Result<()>;
    fn stop(&mut self);
}

pub fn create_listener() -> Result<Box<dyn KeyListener>> {
    #[cfg(target_os = "linux")]
    {
        Ok(Box::new(linux::LinuxEvdevListener::new()))
    }
    #[cfg(windows)]
    {
        Ok(Box::new(windows::WindowsHookListener::new()))
    }
    #[cfg(target_os = "macos")]
    {
        Ok(Box::new(macos::MacosTapListener::new()))
    }
    #[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
    {
        Err(crate::Error::new(
            "Unsupported platform. Thocci supports Linux, Windows, and macOS.",
        ))
    }
}
