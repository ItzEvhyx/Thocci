//! Localhost TCP control protocol shared by the CLI, daemon, and Tauri UI.
//!
//! One newline-terminated command per connection:
//!   `ping`             -> `OK pong`
//!   `status`           -> `OK running volume=<0-100> pack=<id>`
//!   `volume <0-100>`   -> `OK volume=<0-100>`
//!   `pack <id>`        -> `OK pack=<id>`
//!   `stop`             -> `OK stopping`

use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
#[cfg(unix)]
use std::os::fd::AsRawFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::paths::{
    port_file, read_control_port, remove_if_exists, write_control_port, CONTROL_HOST, CONTROL_PORT,
};
use crate::{Error, Result};

pub type CommandHandler = Arc<dyn Fn(&str) -> String + Send + Sync + 'static>;

/// Send a single command to the running daemon and return its trimmed reply.
pub fn send_command(command: &str, timeout: Duration) -> Option<String> {
    send_command_to_port(read_control_port(), command, timeout)
}

pub fn send_command_to_port(port: u16, command: &str, timeout: Duration) -> Option<String> {
    let addr: SocketAddr = format!("{CONTROL_HOST}:{port}").parse().ok()?;
    let mut stream = TcpStream::connect_timeout(&addr, timeout).ok()?;
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));

    let payload = format!("{}\n", command.trim());
    stream.write_all(payload.as_bytes()).ok()?;
    let _ = stream.flush();

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    Some(line.trim().to_owned())
}

pub struct ControlServer {
    port: u16,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl ControlServer {
    pub fn start(handler: CommandHandler) -> Result<Self> {
        Self::start_on(handler, CONTROL_PORT)
    }

    pub fn start_on(handler: CommandHandler, prefer_port: u16) -> Result<Self> {
        let listener = match TcpListener::bind((CONTROL_HOST, prefer_port)) {
            Ok(listener) => listener,
            Err(_) => {
                let fallback = TcpListener::bind((CONTROL_HOST, 0))
                    .map_err(|err| Error::new(format!("Could not bind control port: {err}")))?;
                log::warn!(
                    "Preferred port {prefer_port} busy; using {}",
                    fallback.local_addr().map(|a| a.port()).unwrap_or(0)
                );
                fallback
            }
        };

        let port = listener
            .local_addr()
            .map_err(|err| Error::new(format!("Control listener has no address: {err}")))?
            .port();
        listener
            .set_nonblocking(true)
            .map_err(|err| Error::new(format!("Could not set nonblocking listener: {err}")))?;

        let stop = Arc::new(AtomicBool::new(false));
        let loop_stop = Arc::clone(&stop);
        let thread = thread::Builder::new()
            .name("thocci-control".to_owned())
            .spawn(move || serve_loop(listener, handler, loop_stop))
            .map_err(|err| Error::new(format!("Could not spawn control thread: {err}")))?;

        let mut server = Self {
            port,
            stop,
            thread: Some(thread),
        };
        if let Err(err) = write_control_port(port) {
            server.stop();
            return Err(err.into());
        }

        log::info!("Control server listening on {CONTROL_HOST}:{port}");
        Ok(server)
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
        remove_if_exists(port_file());
    }
}

impl Drop for ControlServer {
    fn drop(&mut self) {
        self.stop();
    }
}

fn wait_for_listener_ready(listener: &TcpListener) {
    #[cfg(unix)]
    {
        let mut pfd = libc::pollfd {
            fd: listener.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // Block in the kernel with zero CPU usage until a client connects, waking
        // only 4x/sec to observe `stop` during shutdown.
        // SAFETY: `pfd` is a valid stack-allocated `pollfd` for the duration of the call.
        let _ = unsafe { libc::poll(&mut pfd, 1, 250) };
    }
    #[cfg(not(unix))]
    {
        let _ = listener;
        thread::sleep(Duration::from_millis(100));
    }
}

fn serve_loop(listener: TcpListener, handler: CommandHandler, stop: Arc<AtomicBool>) {
    while !stop.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((stream, _)) => handle_connection(stream, &handler),
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                wait_for_listener_ready(&listener);
            }
            Err(err) => {
                log::debug!("Control accept error: {err}");
                thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

fn handle_connection(mut stream: TcpStream, handler: &CommandHandler) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));

    let mut line = String::new();
    let read_ok = {
        let mut reader = BufReader::new(&stream);
        reader.read_line(&mut line).is_ok()
    };
    if !read_ok {
        return;
    }

    let reply = handler(line.trim());
    let _ = stream.write_all(format!("{reply}\n").as_bytes());
    let _ = stream.flush();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::test_support;

    #[test]
    fn server_handles_commands_and_stops_cleanly() {
        let _guard = test_support::isolated_state();
        let mut server = ControlServer::start_on(
            Arc::new(|cmd| match cmd {
                "ping" => "OK pong".to_owned(),
                other => format!("ERR unknown:{other}"),
            }),
            0,
        )
        .unwrap();

        let reply = send_command("ping", Duration::from_secs(1));
        assert_eq!(reply.as_deref(), Some("OK pong"));

        let bad = send_command("bogus", Duration::from_secs(1));
        assert_eq!(bad.as_deref(), Some("ERR unknown:bogus"));

        server.stop();
        assert!(!port_file().exists());
    }
}
