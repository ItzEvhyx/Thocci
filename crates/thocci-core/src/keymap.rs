//! Keycode translation.
//!
//! Every input backend reports **Linux evdev keycodes** (`input-event-codes.h`).
//! v1 soundpacks key their `defines` by libuiohook codes, which match Linux for
//! the main block (1..=88) and use extended codes for arrows, Home/End,
//! right Ctrl/Alt, etc. [`uiohook_code`] maps Linux codes to libuiohook codes;
//! [`dom_code_to_linux`] maps v2 DOM `KeyboardEvent.code` strings.

/// Linux `KEY_A`: a device that reports it is treated as a keyboard.
pub const KEY_A: u16 = 30;

/// Physical horizontal stereo zone of a key on a standard keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyZone {
    Left,
    Center,
    Right,
}

/// Map a normalized Linux evdev keycode to its physical stereo zone.
///
/// Uses an explicit position table so platform remapping and keyboard language
/// do not alter left/center/right placement. Navigation cluster and numpad keys
/// map to the right; unclassified keys default to center.
pub fn key_zone(linux_code: u16) -> KeyZone {
    match linux_code {
        // Left: number row (` 1-4), top (Tab Q W E R), home (Caps A S D F),
        // bottom (LShift Z X C, ISO IntlBackslash), bottom controls (LCtrl LMeta LAlt).
        41 | 2..=5 | 15..=19 | 58 | 30..=33 | 42 | 44..=46 | 86 | 29 | 125 | 56 => KeyZone::Left,
        // Center: number row (5-8), top (T Y U), home (G H J),
        // bottom (V B N), bottom controls (Space).
        6..=9 | 20..=22 | 34..=36 | 47..=49 | 57 => KeyZone::Center,
        // Right: number row (9 0 - = Backspace), top (I O P [ ] \),
        // home (K L ; ' Enter), bottom (M , . / RShift),
        // right modifiers & arrows (RCtrl RAlt RMeta Menu, Up/Left/Right/Down),
        // navigation cluster (PrintScreen ScrollLock Pause Home End PgUp PgDn Ins Del),
        // and numpad (NumLock KP* KP7..KP. KPEnter KP/ KP=).
        10..=14
        | 23..=28
        | 37..=40
        | 43
        | 50..=55
        | 69..=83
        | 96..=100
        | 102..=111
        | 117
        | 119
        | 126
        | 127 => KeyZone::Right,
        // Unclassified keys default to center.
        _ => KeyZone::Center,
    }
}

/// Map a DOM `KeyboardEvent.code` to its physical stereo zone using the shared
/// Linux keycode table. Unrecognized or empty codes default to center.
pub fn dom_code_zone(code: &str) -> KeyZone {
    dom_code_to_linux(code)
        .map(key_zone)
        .unwrap_or(KeyZone::Center)
}

/// Keys that never click: Caps/Num/Scroll Lock, Super/Windows/Command keys,
/// and non-typing scroll/zoom/scale events (`KEY_SCALE`, `KEY_SCROLLUP`, `KEY_SCROLLDOWN`).
pub fn is_ignored(linux_code: u16) -> bool {
    matches!(linux_code, 58 | 69 | 70 | 120 | 125 | 126 | 177 | 178)
}

/// Linux keycode -> libuiohook keycode used in v1 pack `defines`.
pub fn uiohook_code(linux_code: u16) -> Option<u32> {
    let code = match linux_code {
        1..=88 => return Some(u32::from(linux_code)),
        96 => 3612,   // KP Enter
        97 => 3613,   // Right Ctrl
        98 => 3637,   // KP /
        99 => 3639,   // Print Screen
        100 => 3640,  // Right Alt
        102 => 3655,  // Home
        103 => 57416, // Up
        104 => 3657,  // Page Up
        105 => 57419, // Left
        106 => 57421, // Right
        107 => 3663,  // End
        108 => 57424, // Down
        109 => 3665,  // Page Down
        110 => 3666,  // Insert
        111 => 3667,  // Delete
        117 => 3597,  // KP =
        119 => 3653,  // Pause
        125 => 3675,  // Left Meta
        126 => 3676,  // Right Meta
        127 => 3677,  // Menu
        183 => 91,    // F13
        184 => 92,    // F14
        185 => 93,    // F15
        _ => return None,
    };
    Some(code)
}

/// Windows low-level hook data -> Linux keycode.
///
/// `scan` is the Set 1 scan code, which equals the Linux keycode for the main
/// block. Keys sent with the 0xE0 prefix (`extended`) reuse scan codes from
/// the numpad, so they need their own table.
pub fn windows_to_linux(vk: u32, scan: u32, extended: bool) -> Option<u16> {
    const VK_PAUSE: u32 = 0x13;
    const VK_NUMLOCK: u32 = 0x90;
    // Pause and NumLock share scan code 0x45; only the virtual key tells them apart.
    match vk {
        VK_PAUSE => return Some(119),
        VK_NUMLOCK => return Some(69),
        _ => {}
    }
    if extended {
        return windows_extended_to_linux(scan);
    }
    match scan {
        1..=88 => Some(scan as u16),
        0x59 => Some(117),                               // KP =
        0x64..=0x6E => Some(183 + (scan - 0x64) as u16), // F13..F23
        0x76 => Some(194),                               // F24
        // Injected input (AutoHotkey, on-screen keyboards) often has no scan code.
        _ => windows_vk_to_linux(vk),
    }
}

fn windows_extended_to_linux(scan: u32) -> Option<u16> {
    let code = match scan {
        0x1C => 96,  // KP Enter
        0x1D => 97,  // Right Ctrl
        0x35 => 98,  // KP /
        0x37 => 99,  // Print Screen
        0x38 => 100, // Right Alt
        0x47 => 102, // Home
        0x48 => 103, // Up
        0x49 => 104, // Page Up
        0x4B => 105, // Left
        0x4D => 106, // Right
        0x4F => 107, // End
        0x50 => 108, // Down
        0x51 => 109, // Page Down
        0x52 => 110, // Insert
        0x53 => 111, // Delete
        0x5B => 125, // Left Windows
        0x5C => 126, // Right Windows
        0x5D => 127, // Menu
        // 0x2A / 0x36 are the "fake shifts" Windows emits around numpad
        // navigation keys; media keys also land here. Neither should click.
        _ => return None,
    };
    Some(code)
}

/// Fallback by virtual-key code when the scan code is missing.
fn windows_vk_to_linux(vk: u32) -> Option<u16> {
    // Linux codes for VK 'A'..='Z' in alphabetical order.
    const LETTERS: [u16; 26] = [
        30, 48, 46, 32, 18, 33, 34, 35, 23, 36, 37, 38, 50, 49, 24, 25, 16, 19, 31, 20, 22, 47, 17,
        45, 21, 44,
    ];
    let code = match vk {
        0x41..=0x5A => LETTERS[(vk - 0x41) as usize],
        0x30 => 11,
        0x31..=0x39 => (vk - 0x31 + 2) as u16,
        0x08 => 14,
        0x09 => 15,
        0x0D => 28,
        0x1B => 1,
        0x20 => 57,
        0x10 | 0xA0 => 42,
        0xA1 => 54,
        0x11 | 0xA2 => 29,
        0xA3 => 97,
        0x12 | 0xA4 => 56,
        0xA5 => 100,
        0x21 => 104,
        0x22 => 109,
        0x23 => 107,
        0x24 => 102,
        0x25 => 105,
        0x26 => 103,
        0x27 => 106,
        0x28 => 108,
        0x2C => 99,
        0x2D => 110,
        0x2E => 111,
        0x70..=0x79 => (vk - 0x70 + 59) as u16,
        0x7A => 87,
        0x7B => 88,
        0xBA => 39,
        0xBB => 13,
        0xBC => 51,
        0xBD => 12,
        0xBE => 52,
        0xBF => 53,
        0xC0 => 41,
        0xDB => 26,
        0xDC => 43,
        0xDD => 27,
        0xDE => 40,
        _ => return None,
    };
    Some(code)
}

/// macOS virtual keycode (`kVK_*`, ANSI layout positions) -> Linux keycode.
pub fn macos_to_linux(keycode: u16) -> Option<u16> {
    let code = match keycode {
        0x00 => 30,  // A
        0x01 => 31,  // S
        0x02 => 32,  // D
        0x03 => 33,  // F
        0x04 => 35,  // H
        0x05 => 34,  // G
        0x06 => 44,  // Z
        0x07 => 45,  // X
        0x08 => 46,  // C
        0x09 => 47,  // V
        0x0A => 86,  // ISO section
        0x0B => 48,  // B
        0x0C => 16,  // Q
        0x0D => 17,  // W
        0x0E => 18,  // E
        0x0F => 19,  // R
        0x10 => 21,  // Y
        0x11 => 20,  // T
        0x12 => 2,   // 1
        0x13 => 3,   // 2
        0x14 => 4,   // 3
        0x15 => 5,   // 4
        0x16 => 7,   // 6
        0x17 => 6,   // 5
        0x18 => 13,  // =
        0x19 => 10,  // 9
        0x1A => 8,   // 7
        0x1B => 12,  // -
        0x1C => 9,   // 8
        0x1D => 11,  // 0
        0x1E => 27,  // ]
        0x1F => 24,  // O
        0x20 => 22,  // U
        0x21 => 26,  // [
        0x22 => 23,  // I
        0x23 => 25,  // P
        0x24 => 28,  // Return
        0x25 => 38,  // L
        0x26 => 36,  // J
        0x27 => 40,  // '
        0x28 => 37,  // K
        0x29 => 39,  // ;
        0x2A => 43,  // backslash
        0x2B => 51,  // ,
        0x2C => 53,  // /
        0x2D => 49,  // N
        0x2E => 50,  // M
        0x2F => 52,  // .
        0x30 => 15,  // Tab
        0x31 => 57,  // Space
        0x32 => 41,  // `
        0x33 => 14,  // Delete (backspace)
        0x35 => 1,   // Escape
        0x36 => 126, // Right Command
        0x37 => 125, // Command
        0x38 => 42,  // Shift
        0x39 => 58,  // Caps Lock
        0x3A => 56,  // Option
        0x3B => 29,  // Control
        0x3C => 54,  // Right Shift
        0x3D => 100, // Right Option
        0x3E => 97,  // Right Control
        0x40 => 187, // F17
        0x41 => 83,  // KP .
        0x43 => 55,  // KP *
        0x45 => 78,  // KP +
        0x47 => 69,  // KP Clear (NumLock position)
        0x4B => 98,  // KP /
        0x4C => 96,  // KP Enter
        0x4E => 74,  // KP -
        0x4F => 188, // F18
        0x50 => 189, // F19
        0x51 => 117, // KP =
        0x52 => 82,  // KP 0
        0x53 => 79,  // KP 1
        0x54 => 80,  // KP 2
        0x55 => 81,  // KP 3
        0x56 => 75,  // KP 4
        0x57 => 76,  // KP 5
        0x58 => 77,  // KP 6
        0x59 => 71,  // KP 7
        0x5A => 190, // F20
        0x5B => 72,  // KP 8
        0x5C => 73,  // KP 9
        0x60 => 63,  // F5
        0x61 => 64,  // F6
        0x62 => 65,  // F7
        0x63 => 61,  // F3
        0x64 => 66,  // F8
        0x65 => 67,  // F9
        0x67 => 87,  // F11
        0x69 => 183, // F13
        0x6A => 186, // F16
        0x6B => 184, // F14
        0x6D => 68,  // F10
        0x6E => 127, // Menu
        0x6F => 88,  // F12
        0x71 => 185, // F15
        0x72 => 110, // Help (Insert position)
        0x73 => 102, // Home
        0x74 => 104, // Page Up
        0x75 => 111, // Forward Delete
        0x76 => 62,  // F4
        0x77 => 107, // End
        0x78 => 60,  // F2
        0x79 => 109, // Page Down
        0x7A => 59,  // F1
        0x7B => 105, // Left
        0x7C => 106, // Right
        0x7D => 108, // Down
        0x7E => 103, // Up
        _ => return None,
    };
    Some(code)
}

/// DOM `KeyboardEvent.code` name (used by v2 soundpacks) -> Linux keycode.
pub fn dom_code_to_linux(name: &str) -> Option<u16> {
    let code = match name {
        "Escape" => 1,
        "Digit1" => 2,
        "Digit2" => 3,
        "Digit3" => 4,
        "Digit4" => 5,
        "Digit5" => 6,
        "Digit6" => 7,
        "Digit7" => 8,
        "Digit8" => 9,
        "Digit9" => 10,
        "Digit0" => 11,
        "Minus" => 12,
        "Equal" => 13,
        "Backspace" => 14,
        "Tab" => 15,
        "KeyQ" => 16,
        "KeyW" => 17,
        "KeyE" => 18,
        "KeyR" => 19,
        "KeyT" => 20,
        "KeyY" => 21,
        "KeyU" => 22,
        "KeyI" => 23,
        "KeyO" => 24,
        "KeyP" => 25,
        "BracketLeft" => 26,
        "BracketRight" => 27,
        "Enter" => 28,
        "ControlLeft" => 29,
        "KeyA" => 30,
        "KeyS" => 31,
        "KeyD" => 32,
        "KeyF" => 33,
        "KeyG" => 34,
        "KeyH" => 35,
        "KeyJ" => 36,
        "KeyK" => 37,
        "KeyL" => 38,
        "Semicolon" => 39,
        "Quote" => 40,
        "Backquote" => 41,
        "ShiftLeft" => 42,
        "Backslash" => 43,
        "KeyZ" => 44,
        "KeyX" => 45,
        "KeyC" => 46,
        "KeyV" => 47,
        "KeyB" => 48,
        "KeyN" => 49,
        "KeyM" => 50,
        "Comma" => 51,
        "Period" => 52,
        "Slash" => 53,
        "ShiftRight" => 54,
        "NumpadMultiply" => 55,
        "AltLeft" => 56,
        "Space" => 57,
        "CapsLock" => 58,
        "F1" => 59,
        "F2" => 60,
        "F3" => 61,
        "F4" => 62,
        "F5" => 63,
        "F6" => 64,
        "F7" => 65,
        "F8" => 66,
        "F9" => 67,
        "F10" => 68,
        "NumLock" => 69,
        "ScrollLock" => 70,
        "Numpad7" => 71,
        "Numpad8" => 72,
        "Numpad9" => 73,
        "NumpadSubtract" => 74,
        "Numpad4" => 75,
        "Numpad5" => 76,
        "Numpad6" => 77,
        "NumpadAdd" => 78,
        "Numpad1" => 79,
        "Numpad2" => 80,
        "Numpad3" => 81,
        "Numpad0" => 82,
        "NumpadDecimal" => 83,
        "IntlBackslash" => 86,
        "F11" => 87,
        "F12" => 88,
        "NumpadEnter" => 96,
        "ControlRight" => 97,
        "NumpadDivide" => 98,
        "PrintScreen" => 99,
        "AltRight" => 100,
        "Home" => 102,
        "ArrowUp" => 103,
        "PageUp" => 104,
        "ArrowLeft" => 105,
        "ArrowRight" => 106,
        "End" => 107,
        "ArrowDown" => 108,
        "PageDown" => 109,
        "Insert" => 110,
        "Delete" => 111,
        "NumpadEqual" => 117,
        "Pause" => 119,
        "MetaLeft" => 125,
        "MetaRight" => 126,
        "ContextMenu" => 127,
        "F13" => 183,
        "F14" => 184,
        "F15" => 185,
        _ => return None,
    };
    Some(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn main_block_is_identity() {
        for code in 1..=88u16 {
            assert_eq!(uiohook_code(code), Some(u32::from(code)));
        }
    }

    #[test]
    fn arrows_map_to_uiohook_codes() {
        assert_eq!(uiohook_code(103), Some(57416));
        assert_eq!(uiohook_code(105), Some(57419));
        assert_eq!(uiohook_code(106), Some(57421));
        assert_eq!(uiohook_code(108), Some(57424));
        assert_eq!(uiohook_code(500), None);
    }

    #[test]
    fn windows_main_block_uses_scan_codes() {
        assert_eq!(windows_to_linux(0x41, 0x1E, false), Some(30)); // A
        assert_eq!(windows_to_linux(0x0D, 0x1C, false), Some(28)); // Enter
        assert_eq!(windows_to_linux(0x0D, 0x1C, true), Some(96)); // KP Enter
    }

    #[test]
    fn windows_extended_keys_do_not_alias_numpad() {
        assert_eq!(windows_to_linux(0x26, 0x48, true), Some(103)); // Up arrow
        assert_eq!(windows_to_linux(0x68, 0x48, false), Some(72)); // KP 8
        assert_eq!(windows_to_linux(0xA0, 0x2A, true), None); // fake shift
    }

    #[test]
    fn windows_pause_and_numlock_use_virtual_key() {
        assert_eq!(windows_to_linux(0x13, 0x45, false), Some(119));
        assert_eq!(windows_to_linux(0x90, 0x45, true), Some(69));
    }

    #[test]
    fn windows_falls_back_to_virtual_key() {
        assert_eq!(windows_to_linux(0x5A, 0, false), Some(44)); // Z
        assert_eq!(windows_to_linux(0x39, 0, false), Some(10)); // 9
        assert_eq!(windows_to_linux(0x7B, 0, false), Some(88)); // F12
    }

    #[test]
    fn macos_letters_and_arrows() {
        assert_eq!(macos_to_linux(0x00), Some(30));
        assert_eq!(macos_to_linux(0x31), Some(57));
        assert_eq!(macos_to_linux(0x7E), Some(103));
        assert_eq!(macos_to_linux(0x3F), None); // Fn
    }

    #[test]
    fn ignored_keys_match_mechy() {
        for code in [58, 69, 70, 120, 125, 126, 177, 178] {
            assert!(is_ignored(code));
        }
        assert!(!is_ignored(KEY_A));
    }

    #[test]
    fn stereo_key_zones_match_spec_across_rows_and_fallbacks() {
        // Left zone across all five rows (including ISO IntlBackslash)
        for code in [
            "Backquote",
            "Digit1",
            "Digit4",
            "Tab",
            "KeyQ",
            "KeyR",
            "CapsLock",
            "KeyA",
            "KeyF",
            "ShiftLeft",
            "KeyZ",
            "KeyC",
            "IntlBackslash",
            "ControlLeft",
            "MetaLeft",
            "AltLeft",
        ] {
            assert_eq!(dom_code_zone(code), KeyZone::Left, "{code} should be Left");
        }

        // Center zone across all five rows
        for code in [
            "Digit5", "Digit8", "KeyT", "KeyU", "KeyG", "KeyH", "KeyJ", "KeyV", "KeyB", "KeyN",
            "Space",
        ] {
            assert_eq!(
                dom_code_zone(code),
                KeyZone::Center,
                "{code} should be Center"
            );
        }

        // Right zone including modifiers, arrows, nav cluster, and numpad
        for code in [
            "Digit9",
            "Digit0",
            "Minus",
            "Equal",
            "Backspace",
            "KeyI",
            "KeyP",
            "BracketLeft",
            "BracketRight",
            "Backslash",
            "KeyK",
            "KeyL",
            "Semicolon",
            "Quote",
            "Enter",
            "KeyM",
            "Comma",
            "Period",
            "Slash",
            "ShiftRight",
            "ControlRight",
            "AltRight",
            "MetaRight",
            "ContextMenu",
            "ArrowLeft",
            "ArrowRight",
            "ArrowUp",
            "ArrowDown",
            "Home",
            "End",
            "PageUp",
            "PageDown",
            "Insert",
            "Delete",
            "PrintScreen",
            "Pause",
            "Numpad0",
            "Numpad5",
            "NumpadEnter",
            "NumpadAdd",
        ] {
            assert_eq!(
                dom_code_zone(code),
                KeyZone::Right,
                "{code} should be Right"
            );
        }

        // Unclassified & unknown keys default to Center
        assert_eq!(dom_code_zone("Escape"), KeyZone::Center);
        assert_eq!(dom_code_zone("F1"), KeyZone::Center);
        assert_eq!(dom_code_zone("UnknownKey"), KeyZone::Center);
        assert_eq!(dom_code_zone(""), KeyZone::Center);
        assert_eq!(key_zone(999), KeyZone::Center);
    }
}
