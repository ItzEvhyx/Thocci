//! Cross-platform global keyboard capture.
//!
//! Each platform backend delivers [`KeyEvent`] (`Down` / `Up`) with Linux evdev
//! keycodes, ignores OS key auto-repeat and synthetic/injected trackpad gesture
//! events on every OS, and skips non-typing scroll/scale codes while ensuring
//! every physical keyboard key (including `CapsLock`, `NumLock`, `ScrollLock`,
//! and `Super`/`Win`/`Cmd`) plays its sound.

use std::collections::HashSet;
use std::sync::Arc;

use crate::keymap;
use crate::Result;

/// Windows `KBDLLHOOKSTRUCT.flags` bitmasks relevant to input filtering.
pub const WIN_LLKHF_EXTENDED: u32 = 0x0000_0001;
pub const WIN_LLKHF_LOWER_IL_INJECTED: u32 = 0x0000_0002;
pub const WIN_LLKHF_INJECTED: u32 = 0x0000_0010;
pub const WIN_LLKHF_UP: u32 = 0x0000_0080;

/// Windows low-level keyboard hook message identifiers (`WM_*`).
pub const WIN_WM_KEYDOWN: u32 = 0x0100;
pub const WIN_WM_KEYUP: u32 = 0x0101;
pub const WIN_WM_SYSKEYDOWN: u32 = 0x0104;
pub const WIN_WM_SYSKEYUP: u32 = 0x0105;

/// Snapshot of a Windows `WH_KEYBOARD_LL` callback payload (`wParam` + `KBDLLHOOKSTRUCT`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowsHookEvent {
    pub msg: u32,
    pub vk_code: u32,
    pub scan_code: u32,
    pub flags: u32,
}

/// Stateful filter and translator for Windows `WH_KEYBOARD_LL` events.
///
/// Kept platform-independent so Windows hook filtering and press/release state
/// tracking are exercised by cross-platform unit tests.
#[derive(Debug, Default)]
pub struct WindowsKeyState {
    pressed: HashSet<u16>,
}

impl WindowsKeyState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.pressed.clear();
    }

    pub fn process(&mut self, ev: WindowsHookEvent) -> Option<KeyEvent> {
        // Ignore software/OS-injected keyboard events (Windows Precision Touchpad
        // pinch-to-zoom Ctrl synthesis, Synaptics/ELAN gesture drivers, multi-finger
        // swipe shortcuts, and SendInput/keybd_event macros).
        const INJECTED_MASK: u32 = WIN_LLKHF_INJECTED | WIN_LLKHF_LOWER_IL_INJECTED;
        if (ev.flags & INJECTED_MASK) != 0 {
            return None;
        }

        // Physical keyboards in WH_KEYBOARD_LL always emit sided modifier VKs
        // (VK_LSHIFT..=VK_RMENU, 0xA0..=0xA5) with a non-zero hardware scan code.
        // Generic unsided modifiers (VK_SHIFT/VK_CONTROL/VK_MENU) or zero-scancode
        // modifier events originate from synthetic/gesture drivers (e.g. trackpad zoom).
        const VK_SHIFT: u32 = 0x10;
        const VK_CONTROL: u32 = 0x11;
        const VK_MENU: u32 = 0x12;
        const VK_LSHIFT: u32 = 0xA0;
        const VK_RMENU: u32 = 0xA5;
        if matches!(ev.vk_code, VK_SHIFT | VK_CONTROL | VK_MENU) {
            return None;
        }
        if ev.scan_code == 0 && (VK_LSHIFT..=VK_RMENU).contains(&ev.vk_code) {
            return None;
        }

        let extended = (ev.flags & WIN_LLKHF_EXTENDED) != 0;
        let linux_code = keymap::windows_to_linux(ev.vk_code, ev.scan_code, extended)?;
        if keymap::is_ignored(linux_code) {
            return None;
        }
        match ev.msg {
            WIN_WM_KEYDOWN | WIN_WM_SYSKEYDOWN => {
                if !self.pressed.insert(linux_code) {
                    return None;
                }
                Some(KeyEvent {
                    code: linux_code,
                    kind: KeyEventKind::Down,
                })
            }
            WIN_WM_KEYUP | WIN_WM_SYSKEYUP => {
                if !self.pressed.remove(&linux_code) {
                    return None;
                }
                Some(KeyEvent {
                    code: linux_code,
                    kind: KeyEventKind::Up,
                })
            }
            _ => None,
        }
    }
}

/// macOS `CGEventFlags` bitmasks relevant to modifier key transitions.
pub const MAC_FLAG_ALPHA_SHIFT: u64 = 0x0001_0000;
pub const MAC_FLAG_SHIFT: u64 = 0x0002_0000;
pub const MAC_FLAG_CONTROL: u64 = 0x0004_0000;
pub const MAC_FLAG_ALTERNATE: u64 = 0x0008_0000;
pub const MAC_FLAG_COMMAND: u64 = 0x0010_0000;

/// Stateful tracker for macOS `CGEventType::FlagsChanged` modifier events.
///
/// Standard modifiers (Shift/Control/Option/Command) emit `Down` when their
/// flag bit rises and `Up` when it clears. Caps Lock (`0x39`) is a latching
/// toggle on macOS (`CGEventFlagAlphaShift`), so every toggle transition
/// (turning Caps Lock ON or OFF) emits a physical key press (`Down`).
#[derive(Debug, Default)]
pub struct MacosModifierState {
    prev_flags: u64,
    initialized: bool,
}

impl MacosModifierState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn process(&mut self, mac_key: u16, curr_flags: u64) -> Option<KeyEvent> {
        let had_prev = self.initialized;
        let prev_flags = self.prev_flags;
        self.prev_flags = curr_flags;
        self.initialized = true;

        let kind = if mac_key == 0x39 {
            let was_on = (prev_flags & MAC_FLAG_ALPHA_SHIFT) != 0;
            let is_on = (curr_flags & MAC_FLAG_ALPHA_SHIFT) != 0;
            if !had_prev || was_on != is_on {
                Some(KeyEventKind::Down)
            } else {
                None
            }
        } else {
            let mask = match mac_key {
                0x38 | 0x3C => MAC_FLAG_SHIFT,
                0x3B | 0x3E => MAC_FLAG_CONTROL,
                0x3A | 0x3D => MAC_FLAG_ALTERNATE,
                0x37 | 0x36 => MAC_FLAG_COMMAND,
                _ => return None,
            };
            let was_down = (prev_flags & mask) != 0;
            let is_down = (curr_flags & mask) != 0;
            match (was_down, is_down) {
                (false, true) => Some(KeyEventKind::Down),
                (true, false) => Some(KeyEventKind::Up),
                _ => None,
            }
        }?;

        let linux_code = keymap::macos_to_linux(mac_key)?;
        if keymap::is_ignored(linux_code) {
            return None;
        }
        Some(KeyEvent {
            code: linux_code,
            kind,
        })
    }
}

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyEventKind {
    Down,
    Up,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent {
    pub code: u16,
    pub kind: KeyEventKind,
}

pub type KeyCallback = Arc<dyn Fn(KeyEvent) + Send + Sync + 'static>;

pub trait KeyListener: Send {
    fn start(&mut self, on_key: KeyCallback) -> Result<()>;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_trackpad_pinch_zoom_injected_ctrl_does_not_click() {
        let mut state = WindowsKeyState::new();

        // Windows Precision Touchpad pinch-to-zoom synthesizes VK_CONTROL (0x11)
        // with LLKHF_INJECTED set (scan_code 0 or 0x1D) around WM_MOUSEWHEEL.
        let ptp_down = WindowsHookEvent {
            msg: WIN_WM_KEYDOWN,
            vk_code: 0x11, // VK_CONTROL
            scan_code: 0,
            flags: WIN_LLKHF_INJECTED,
        };
        let ptp_up = WindowsHookEvent {
            msg: WIN_WM_KEYUP,
            vk_code: 0x11,
            scan_code: 0,
            flags: WIN_LLKHF_INJECTED | WIN_LLKHF_UP,
        };
        assert_eq!(state.process(ptp_down), None);
        assert_eq!(state.process(ptp_up), None);

        // Some drivers (Synaptics/ELAN) inject sided VK_LCONTROL + +/- with LLKHF_INJECTED
        // or LLKHF_LOWER_IL_INJECTED.
        for ev in [
            WindowsHookEvent {
                msg: WIN_WM_KEYDOWN,
                vk_code: 0xA2, // VK_LCONTROL
                scan_code: 0x1D,
                flags: WIN_LLKHF_INJECTED,
            },
            WindowsHookEvent {
                msg: WIN_WM_KEYDOWN,
                vk_code: 0xBB, // VK_OEM_PLUS
                scan_code: 0x0D,
                flags: WIN_LLKHF_INJECTED,
            },
            WindowsHookEvent {
                msg: WIN_WM_KEYUP,
                vk_code: 0xBB,
                scan_code: 0x0D,
                flags: WIN_LLKHF_INJECTED | WIN_LLKHF_UP,
            },
            WindowsHookEvent {
                msg: WIN_WM_KEYUP,
                vk_code: 0xA2,
                scan_code: 0x1D,
                flags: WIN_LLKHF_INJECTED | WIN_LLKHF_LOWER_IL_INJECTED | WIN_LLKHF_UP,
            },
        ] {
            assert_eq!(
                state.process(ev),
                None,
                "injected zoom event must be ignored: {ev:?}"
            );
        }
    }

    #[test]
    fn windows_trackpad_synthetic_non_injected_ctrl_and_stray_keyup_do_not_click() {
        let mut state = WindowsKeyState::new();

        // Kernel touchpad drivers that omit LLKHF_INJECTED still emit generic non-sided
        // VK_CONTROL (0x11) or zero-scancode modifiers, never a real physical hardware key.
        assert_eq!(
            state.process(WindowsHookEvent {
                msg: WIN_WM_KEYDOWN,
                vk_code: 0x11, // VK_CONTROL (non-sided)
                scan_code: 0x1D,
                flags: 0,
            }),
            None
        );
        assert_eq!(
            state.process(WindowsHookEvent {
                msg: WIN_WM_KEYDOWN,
                vk_code: 0xA2, // VK_LCONTROL with missing hardware scan code
                scan_code: 0,
                flags: 0,
            }),
            None
        );
        // Stray WM_KEYUP without a prior accepted physical keydown must not emit Up.
        assert_eq!(
            state.process(WindowsHookEvent {
                msg: WIN_WM_KEYUP,
                vk_code: 0xA2,
                scan_code: 0x1D,
                flags: WIN_LLKHF_UP,
            }),
            None
        );
    }

    #[test]
    fn windows_physical_keyboard_including_ctrl_and_repeat_still_works() {
        let mut state = WindowsKeyState::new();

        // Physical Left Ctrl (VK_LCONTROL = 0xA2, scanCode = 0x1D, flags = 0)
        assert_eq!(
            state.process(WindowsHookEvent {
                msg: WIN_WM_KEYDOWN,
                vk_code: 0xA2,
                scan_code: 0x1D,
                flags: 0,
            }),
            Some(KeyEvent {
                code: 29,
                kind: KeyEventKind::Down,
            })
        );
        // Auto-repeat while held must be ignored
        assert_eq!(
            state.process(WindowsHookEvent {
                msg: WIN_WM_KEYDOWN,
                vk_code: 0xA2,
                scan_code: 0x1D,
                flags: 0,
            }),
            None
        );
        // Physical 'A' pressed while Ctrl is held (e.g. Ctrl+A)
        assert_eq!(
            state.process(WindowsHookEvent {
                msg: WIN_WM_KEYDOWN,
                vk_code: 0x41,
                scan_code: 0x1E,
                flags: 0,
            }),
            Some(KeyEvent {
                code: 30,
                kind: KeyEventKind::Down,
            })
        );
        assert_eq!(
            state.process(WindowsHookEvent {
                msg: WIN_WM_KEYUP,
                vk_code: 0x41,
                scan_code: 0x1E,
                flags: WIN_LLKHF_UP,
            }),
            Some(KeyEvent {
                code: 30,
                kind: KeyEventKind::Up,
            })
        );
        assert_eq!(
            state.process(WindowsHookEvent {
                msg: WIN_WM_KEYUP,
                vk_code: 0xA2,
                scan_code: 0x1D,
                flags: WIN_LLKHF_UP,
            }),
            Some(KeyEvent {
                code: 29,
                kind: KeyEventKind::Up,
            })
        );
        // Physical Right Ctrl (extended scan code 0x1D)
        assert_eq!(
            state.process(WindowsHookEvent {
                msg: WIN_WM_KEYDOWN,
                vk_code: 0xA3,
                scan_code: 0x1D,
                flags: WIN_LLKHF_EXTENDED,
            }),
            Some(KeyEvent {
                code: 97,
                kind: KeyEventKind::Down,
            })
        );
    }

    #[test]
    fn windows_capslock_locks_and_meta_keys_emit_key_events() {
        let mut state = WindowsKeyState::new();

        for (vk_code, scan_code, flags, expected_code) in [
            (0x14, 0x3A, 0, 58),                   // Caps Lock
            (0x90, 0x45, WIN_LLKHF_EXTENDED, 69),  // Num Lock
            (0x91, 0x46, 0, 70),                   // Scroll Lock
            (0x5B, 0x5B, WIN_LLKHF_EXTENDED, 125), // Left Win / Super
            (0x5C, 0x5C, WIN_LLKHF_EXTENDED, 126), // Right Win / Super
        ] {
            assert_eq!(
                state.process(WindowsHookEvent {
                    msg: WIN_WM_KEYDOWN,
                    vk_code,
                    scan_code,
                    flags,
                }),
                Some(KeyEvent {
                    code: expected_code,
                    kind: KeyEventKind::Down,
                }),
                "keydown for vk=0x{vk_code:02X} scan=0x{scan_code:02X} should emit code {expected_code}"
            );
            assert_eq!(
                state.process(WindowsHookEvent {
                    msg: WIN_WM_KEYUP,
                    vk_code,
                    scan_code,
                    flags: flags | WIN_LLKHF_UP,
                }),
                Some(KeyEvent {
                    code: expected_code,
                    kind: KeyEventKind::Up,
                }),
                "keyup for vk=0x{vk_code:02X} scan=0x{scan_code:02X} should emit code {expected_code}"
            );
        }
    }

    #[test]
    fn macos_capslock_and_modifiers_emit_appropriate_key_events() {
        let mut state = MacosModifierState::new();

        // Caps Lock toggle ON -> Down
        assert_eq!(
            state.process(0x39, MAC_FLAG_ALPHA_SHIFT),
            Some(KeyEvent {
                code: 58,
                kind: KeyEventKind::Down,
            })
        );
        // Duplicate FlagsChanged with unchanged AlphaShift bit -> ignored
        assert_eq!(state.process(0x39, MAC_FLAG_ALPHA_SHIFT), None);
        // Caps Lock toggle OFF -> Down (toggling off is a physical key press)
        assert_eq!(
            state.process(0x39, 0),
            Some(KeyEvent {
                code: 58,
                kind: KeyEventKind::Down,
            })
        );

        // Command press & release -> Down / Up for Left Meta (125)
        assert_eq!(
            state.process(0x37, MAC_FLAG_COMMAND),
            Some(KeyEvent {
                code: 125,
                kind: KeyEventKind::Down,
            })
        );
        assert_eq!(
            state.process(0x37, 0),
            Some(KeyEvent {
                code: 125,
                kind: KeyEventKind::Up,
            })
        );

        // Starting with Caps Lock already ON: first press turns it OFF (flags=0) -> still Down
        let mut already_on = MacosModifierState::new();
        assert_eq!(
            already_on.process(0x39, 0),
            Some(KeyEvent {
                code: 58,
                kind: KeyEventKind::Down,
            })
        );
    }
}
