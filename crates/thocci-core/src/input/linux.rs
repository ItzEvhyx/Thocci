//! Linux keyboard capture via `/dev/input/event*` (`evdev`).

use std::fs;
use std::io::ErrorKind;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use evdev::{Device, EventType, KeyCode};

use crate::input::{KeyCallback, KeyEvent, KeyEventKind, KeyListener};
use crate::keymap::{self, KEY_A};
use crate::{Error, Result};

const POLL_TIMEOUT_MS: i32 = 250;

pub struct LinuxEvdevListener {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl LinuxEvdevListener {
    pub fn new() -> Self {
        Self {
            stop: Arc::new(AtomicBool::new(false)),
            thread: None,
        }
    }
}

impl KeyListener for LinuxEvdevListener {
    fn start(&mut self, on_key: KeyCallback) -> Result<()> {
        let devices = discover_keyboards()?;
        self.stop.store(false, Ordering::Release);
        let stop = Arc::clone(&self.stop);

        let handle = thread::Builder::new()
            .name("thocci-evdev".to_owned())
            .spawn(move || poll_loop(devices, on_key, stop))
            .map_err(|err| Error::new(format!("Could not spawn evdev thread: {err}")))?;
        self.thread = Some(handle);
        Ok(())
    }

    fn stop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for LinuxEvdevListener {
    fn drop(&mut self) {
        self.stop();
    }
}

struct OpenedKeyboard {
    path: PathBuf,
    device: Device,
}

fn discover_keyboards() -> Result<Vec<OpenedKeyboard>> {
    let mut devices = Vec::new();
    let mut denied = 0usize;

    let mut entries: Vec<PathBuf> = fs::read_dir("/dev/input")
        .map_err(|err| Error::new(format!("Could not read /dev/input: {err}")))?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| is_event_node(path))
        .collect();
    entries.sort();

    for path in entries {
        let device = match Device::open(&path) {
            Ok(dev) => dev,
            Err(err) if err.kind() == ErrorKind::PermissionDenied => {
                denied += 1;
                log::debug!("Skipping {}: permission denied", path.display());
                continue;
            }
            Err(err) => {
                log::debug!("Skipping {}: {err}", path.display());
                continue;
            }
        };

        let is_keyboard = device
            .supported_keys()
            .map(|keys| keys.contains(KeyCode::new(KEY_A)))
            .unwrap_or(false);
        if !is_keyboard {
            continue;
        }

        if let Err(err) = device.set_nonblocking(true) {
            log::debug!("Skipping {}: nonblocking failed: {err}", path.display());
            continue;
        }

        let name = device.name().unwrap_or("unknown");
        log::info!("Listening on {} ({name})", path.display());
        devices.push(OpenedKeyboard { path, device });
    }

    if devices.is_empty() {
        if denied > 0 {
            return Err(Error::new(format!(
                "Permission denied on {denied} input device(s). Add your user to the 'input' group (`sudo usermod -aG input $USER`), then log out and back in."
            )));
        }
        return Err(Error::new(
            "No keyboard devices found under /dev/input. Add your user to the 'input' group (`sudo usermod -aG input $USER`), then log out and back in.",
        ));
    }

    Ok(devices)
}

fn is_event_node(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.starts_with("event"))
        .unwrap_or(false)
}

fn poll_loop(mut devices: Vec<OpenedKeyboard>, on_key: KeyCallback, stop: Arc<AtomicBool>) {
    while !stop.load(Ordering::Acquire) {
        if devices.is_empty() {
            log::error!("All keyboard devices disconnected");
            break;
        }

        let mut fds: Vec<libc::pollfd> = devices
            .iter()
            .map(|kb| libc::pollfd {
                fd: kb.device.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            })
            .collect();

        // SAFETY: `fds` points to a contiguous slice of `pollfd` valid for the call.
        let ready =
            unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, POLL_TIMEOUT_MS) };
        if ready < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == ErrorKind::Interrupted {
                continue;
            }
            log::error!("evdev poll failed: {err}");
            break;
        }
        if ready == 0 {
            continue;
        }

        let mut disconnected = Vec::new();
        for (idx, pfd) in fds.iter().enumerate() {
            if (pfd.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL)) != 0 {
                disconnected.push(idx);
                continue;
            }
            if (pfd.revents & libc::POLLIN) == 0 {
                continue;
            }
            if !drain_device(&mut devices[idx], &on_key) {
                disconnected.push(idx);
            }
        }

        for idx in disconnected.into_iter().rev() {
            let lost = devices.remove(idx);
            log::warn!("Lost keyboard device {}", lost.path.display());
        }
    }
}

fn drain_device(kb: &mut OpenedKeyboard, on_key: &KeyCallback) -> bool {
    loop {
        match kb.device.fetch_events() {
            Ok(events) => {
                for ev in events {
                    if ev.event_type() != EventType::KEY {
                        continue;
                    }
                    // value: 1 = initial press, 0 = release, 2 = auto-repeat (ignored).
                    let kind = match ev.value() {
                        1 => KeyEventKind::Down,
                        0 => KeyEventKind::Up,
                        _ => continue,
                    };
                    let code = ev.code();
                    if keymap::is_ignored(code) {
                        continue;
                    }
                    on_key(KeyEvent { code, kind });
                }
            }
            Err(err) if err.kind() == ErrorKind::WouldBlock => return true,
            Err(err) => {
                log::warn!("Read error on {}: {err}", kb.path.display());
                return false;
            }
        }
    }
}
