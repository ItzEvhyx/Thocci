//! Cross-platform process lifecycle helpers (PID check, terminate, detach).

use std::fs::OpenOptions;
use std::path::Path;
use std::process::{Child, Command, Stdio};

use crate::{Error, Result};

/// Return `true` if a process with `pid` is currently alive.
pub fn pid_is_running(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    platform_pid_is_running(pid)
}

/// Ask `pid` to exit gracefully (`force == false`) or kill it immediately.
pub fn terminate_pid(pid: u32, force: bool) -> Result<()> {
    if pid == 0 {
        return Err(Error::new("Invalid PID 0"));
    }
    platform_terminate_pid(pid, force)
}

/// Spawn a detached background child whose stdout/stderr append to `log_path`.
pub fn spawn_detached(exe: &Path, args: &[String], log_path: &Path) -> Result<Child> {
    let stdout_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)?;
    let stderr_file = stdout_file.try_clone()?;

    let mut cmd = Command::new(exe);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout_file))
        .stderr(Stdio::from(stderr_file));

    configure_detached(&mut cmd);
    cmd.spawn()
        .map_err(|err| Error::new(format!("Failed to start {}: {err}", exe.display())))
}

#[cfg(unix)]
fn platform_pid_is_running(pid: u32) -> bool {
    // SAFETY: `kill(pid, 0)` only checks process existence and sends no signal.
    let rc = unsafe { libc::kill(pid as libc::pid_t, 0) };
    if rc == 0 {
        return true;
    }
    let errno = std::io::Error::last_os_error().raw_os_error();
    // EPERM means the process exists even though we cannot signal it.
    errno == Some(libc::EPERM)
}

#[cfg(unix)]
fn platform_terminate_pid(pid: u32, force: bool) -> Result<()> {
    let sig = if force { libc::SIGKILL } else { libc::SIGTERM };
    // SAFETY: Standard POSIX signal delivery to a known PID.
    let rc = unsafe { libc::kill(pid as libc::pid_t, sig) };
    if rc == 0 {
        return Ok(());
    }
    let err = std::io::Error::last_os_error();
    if err.raw_os_error() == Some(libc::ESRCH) {
        return Ok(());
    }
    Err(Error::new(format!("Could not stop pid {pid}: {err}")))
}

#[cfg(unix)]
fn configure_detached(cmd: &mut Command) {
    use std::os::unix::process::CommandExt;

    // SAFETY: `setsid()` is async-signal-safe and detaches the child from the
    // controlling terminal so closing the shell leaves the daemon running.
    unsafe {
        cmd.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

#[cfg(windows)]
fn platform_pid_is_running(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    const STILL_ACTIVE: u32 = 259;

    // SAFETY: Standard Win32 process handle open/query/close.
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return false;
        }
        let mut exit_code: u32 = 0;
        let ok = GetExitCodeProcess(handle, &mut exit_code);
        CloseHandle(handle);
        ok != 0 && exit_code == STILL_ACTIVE
    }
}

#[cfg(windows)]
fn platform_terminate_pid(pid: u32, _force: bool) -> Result<()> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};

    // SAFETY: Standard Win32 process termination for a known PID.
    unsafe {
        let handle = OpenProcess(PROCESS_TERMINATE, 0, pid);
        if handle.is_null() {
            return Err(Error::new(format!("Cannot open process {pid}")));
        }
        let ok = TerminateProcess(handle, 1);
        CloseHandle(handle);
        if ok == 0 {
            return Err(Error::new(format!("TerminateProcess failed for {pid}")));
        }
    }
    Ok(())
}

#[cfg(windows)]
fn configure_detached(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_process_is_running() {
        assert!(pid_is_running(std::process::id()));
        assert!(!pid_is_running(0));
    }
}
