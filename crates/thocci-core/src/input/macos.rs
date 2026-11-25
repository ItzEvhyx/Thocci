//! macOS global keyboard capture via a listen-only `CGEventTap`.
//!
//! Requires Input Monitoring / Accessibility permission in System Settings ->
//! Privacy & Security. Ignores key auto-repeat and delivers both key press and
//! key release events (including modifier transitions via `FlagsChanged`).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use core_foundation::runloop::{kCFRunLoopCommonModes, kCFRunLoopDefaultMode, CFRunLoop};
use core_graphics::event::{
    CGEventFlags, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
    CGEventType, CallbackResult, EventField,
};

use crate::input::{KeyCallback, KeyEvent, KeyEventKind, KeyListener};
use crate::keymap;
use crate::{Error, Result};

pub struct MacosTapListener {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl MacosTapListener {
    pub fn new() -> Self {
        Self {
            stop: Arc::new(AtomicBool::new(false)),
            thread: None,
        }
    }
}

impl KeyListener for MacosTapListener {
    fn start(&mut self, on_key: KeyCallback) -> Result<()> {
        self.stop.store(false, Ordering::Release);
        let stop = Arc::clone(&self.stop);
        let (ready_tx, ready_rx) = mpsc::channel::<Result<()>>();

        let handle = thread::Builder::new()
            .name("thocci-cgevent".to_owned())
            .spawn(move || tap_thread(on_key, stop, ready_tx))
            .map_err(|err| Error::new(format!("Could not spawn macOS event tap thread: {err}")))?;

        match ready_rx.recv() {
            Ok(Ok(())) => {
                self.thread = Some(handle);
                Ok(())
            }
            Ok(Err(err)) => {
                let _ = handle.join();
                Err(err)
            }
            Err(_) => {
                let _ = handle.join();
                Err(Error::new("macOS event tap thread exited unexpectedly"))
            }
        }
    }

    fn stop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for MacosTapListener {
    fn drop(&mut self) {
        self.stop();
    }
}

fn tap_thread(on_key: KeyCallback, stop: Arc<AtomicBool>, ready_tx: mpsc::Sender<Result<()>>) {
    let prev_flags = Arc::new(AtomicU64::new(0));
    let cb_flags = Arc::clone(&prev_flags);
    let cb = on_key;

    let tap = match CGEventTap::new(
        CGEventTapLocation::HID,
        CGEventTapPlacement::HeadInsertEventTap,
        CGEventTapOptions::ListenOnly,
        vec![
            CGEventType::KeyDown,
            CGEventType::KeyUp,
            CGEventType::FlagsChanged,
        ],
        move |_proxy, etype, event| {
            handle_event(etype, event, &cb_flags, &cb);
            CallbackResult::Keep
        },
    ) {
        Ok(tap) => tap,
        Err(()) => {
            let _ = ready_tx.send(Err(Error::new(
                "Could not create macOS keyboard event tap. Grant Input Monitoring / Accessibility access in System Settings -> Privacy & Security.",
            )));
            return;
        }
    };

    let Ok(source) = tap.mach_port().create_runloop_source(0) else {
        let _ = ready_tx.send(Err(Error::new("Could not create CFRunLoopSource")));
        return;
    };

    let run_loop = CFRunLoop::get_current();
    // SAFETY: `kCFRunLoopCommonModes` is a CoreFoundation constant string.
    run_loop.add_source(&source, unsafe { kCFRunLoopCommonModes });
    tap.enable();

    log::info!("macOS CGEventTap keyboard listener active");
    let _ = ready_tx.send(Ok(()));

    while !stop.load(Ordering::Acquire) {
        // SAFETY: `kCFRunLoopDefaultMode` is a CoreFoundation constant string.
        unsafe {
            CFRunLoop::run_in_mode(kCFRunLoopDefaultMode, Duration::from_millis(100), false);
        }
    }
}

fn handle_event(
    etype: CGEventType,
    event: &core_graphics::event::CGEvent,
    prev_flags: &AtomicU64,
    on_key: &KeyCallback,
) {
    let raw_key = event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE);
    let Ok(mac_key) = u16::try_from(raw_key) else {
        return;
    };

    match etype {
        CGEventType::KeyDown => {
            if event.get_integer_value_field(EventField::KEYBOARD_EVENT_AUTOREPEAT) != 0 {
                return;
            }
            emit_if_mapped(mac_key, KeyEventKind::Down, on_key);
        }
        CGEventType::KeyUp => {
            emit_if_mapped(mac_key, KeyEventKind::Up, on_key);
        }
        CGEventType::FlagsChanged => {
            let current = event.get_flags().bits();
            let previous = prev_flags.swap(current, Ordering::AcqRel);
            if let Some(kind) = modifier_transition(mac_key, previous, current) {
                emit_if_mapped(mac_key, kind, on_key);
            }
        }
        _ => {}
    }
}

fn modifier_transition(mac_key: u16, prev: u64, curr: u64) -> Option<KeyEventKind> {
    let mask = match mac_key {
        0x38 | 0x3C => CGEventFlags::CGEventFlagShift.bits(),
        0x3B | 0x3E => CGEventFlags::CGEventFlagControl.bits(),
        0x3A | 0x3D => CGEventFlags::CGEventFlagAlternate.bits(),
        0x37 | 0x36 => CGEventFlags::CGEventFlagCommand.bits(),
        _ => return None,
    };
    let was_down = (prev & mask) != 0;
    let is_down = (curr & mask) != 0;
    match (was_down, is_down) {
        (false, true) => Some(KeyEventKind::Down),
        (true, false) => Some(KeyEventKind::Up),
        _ => None,
    }
}

fn emit_if_mapped(mac_key: u16, kind: KeyEventKind, on_key: &KeyCallback) {
    if let Some(linux_code) = keymap::macos_to_linux(mac_key) {
        if !keymap::is_ignored(linux_code) {
            on_key(KeyEvent {
                code: linux_code,
                kind,
            });
        }
    }
}
