//! Windows keyboard capture via `SetWindowsHookExW(WH_KEYBOARD_LL)`.
//!
//! Tracks held keys so Windows auto-repeat does not spam clicks while holding a
//! key down, emits both press (`Down`) and release (`Up`) events, and translates
//! Set 1 scan codes + `LLKHF_EXTENDED` flags into Linux evdev keycodes.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};

use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, PostThreadMessageW, SetWindowsHookExW,
    TranslateMessage, UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT, LLKHF_EXTENDED, MSG,
    WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_QUIT, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

use crate::input::{KeyCallback, KeyEvent, KeyEventKind, KeyListener};
use crate::keymap;
use crate::{Error, Result};

struct HookState {
    callback: Option<KeyCallback>,
    pressed: HashSet<u16>,
}

static HOOK_STATE: OnceLock<Mutex<HookState>> = OnceLock::new();

fn hook_state() -> &'static Mutex<HookState> {
    HOOK_STATE.get_or_init(|| {
        Mutex::new(HookState {
            callback: None,
            pressed: HashSet::new(),
        })
    })
}

pub struct WindowsHookListener {
    thread_id: Arc<AtomicU32>,
    thread: Option<JoinHandle<()>>,
}

impl WindowsHookListener {
    pub fn new() -> Self {
        Self {
            thread_id: Arc::new(AtomicU32::new(0)),
            thread: None,
        }
    }
}

impl KeyListener for WindowsHookListener {
    fn start(&mut self, on_key: KeyCallback) -> Result<()> {
        {
            let mut state = hook_state().lock().unwrap_or_else(|e| e.into_inner());
            state.callback = Some(on_key);
            state.pressed.clear();
        }

        let (ready_tx, ready_rx) = mpsc::channel::<Result<()>>();
        let thread_id = Arc::clone(&self.thread_id);

        let handle = thread::Builder::new()
            .name("thocci-winhook".to_owned())
            .spawn(move || hook_thread(thread_id, ready_tx))
            .map_err(|err| Error::new(format!("Could not spawn Windows hook thread: {err}")))?;

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
                Err(Error::new(
                    "Windows keyboard hook thread exited unexpectedly",
                ))
            }
        }
    }

    fn stop(&mut self) {
        let tid = self.thread_id.swap(0, Ordering::AcqRel);
        if tid != 0 {
            // SAFETY: Posting WM_QUIT wakes GetMessageW so the hook thread unhooks and exits.
            unsafe {
                PostThreadMessageW(tid, WM_QUIT, 0, 0);
            }
        }
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
        let mut state = hook_state().lock().unwrap_or_else(|e| e.into_inner());
        state.callback = None;
        state.pressed.clear();
    }
}

impl Drop for WindowsHookListener {
    fn drop(&mut self) {
        self.stop();
    }
}

fn hook_thread(thread_id: Arc<AtomicU32>, ready_tx: mpsc::Sender<Result<()>>) {
    // SAFETY: Standard Win32 low-level keyboard hook setup and message pump.
    unsafe {
        let tid = GetCurrentThreadId();
        let hmod = GetModuleHandleW(std::ptr::null());
        let hook: HHOOK = SetWindowsHookExW(WH_KEYBOARD_LL, Some(low_level_keyboard_proc), hmod, 0);
        if hook.is_null() {
            let err = std::io::Error::last_os_error();
            let _ = ready_tx.send(Err(Error::new(format!("SetWindowsHookExW failed: {err}"))));
            return;
        }

        thread_id.store(tid, Ordering::Release);
        log::info!("Windows low-level keyboard hook active");
        let _ = ready_tx.send(Ok(()));

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        UnhookWindowsHookEx(hook);
    }
}

unsafe extern "system" fn low_level_keyboard_proc(
    code: i32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if code >= 0 && lparam != 0 {
        let info = &*(lparam as *const KBDLLHOOKSTRUCT);
        let msg = wparam as u32;
        let extended = (info.flags & LLKHF_EXTENDED) != 0;

        if let Some(linux_code) = keymap::windows_to_linux(info.vkCode, info.scanCode, extended) {
            if !keymap::is_ignored(linux_code) {
                dispatch_key_event(msg, linux_code);
            }
        }
    }
    CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
}

fn dispatch_key_event(msg: u32, linux_code: u16) {
    let event_and_cb = {
        let mut state = hook_state().lock().unwrap_or_else(|e| e.into_inner());
        match msg {
            WM_KEYDOWN | WM_SYSKEYDOWN => {
                // Ignore OS key auto-repeat while the key is already held down.
                if !state.pressed.insert(linux_code) {
                    return;
                }
                state.callback.clone().map(|cb| (cb, KeyEventKind::Down))
            }
            WM_KEYUP | WM_SYSKEYUP => {
                state.pressed.remove(&linux_code);
                state.callback.clone().map(|cb| (cb, KeyEventKind::Up))
            }
            _ => None,
        }
    };

    if let Some((cb, kind)) = event_and_cb {
        cb(KeyEvent {
            code: linux_code,
            kind,
        });
    }
}
