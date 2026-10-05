//! Client side of the agent's named pipe (used by the CLI and the UI).

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::os::windows::fs::OpenOptionsExt;
use std::time::Duration;

use crate::{PIPE_NAME, Request, Response};

/// FILE_GENERIC_READ | FILE_WRITE_DATA: exactly what the agent's pipe
/// grants to normal users (GENERIC_WRITE would be refused).
const PIPE_ACCESS: u32 = 0x0012_008b;
const ERROR_PIPE_BUSY: i32 = 231;

fn connect() -> std::io::Result<File> {
    let mut last = None;
    // The agent serves one client at a time; retry briefly if it is busy.
    for _ in 0..20 {
        match OpenOptions::new().read(true).access_mode(PIPE_ACCESS).open(PIPE_NAME) {
            Ok(f) => return Ok(f),
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY) => {
                last = Some(e);
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(e),
        }
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("pipe busy")))
}

/// Sends one request and waits for the answer.
pub fn send(request: &Request) -> std::io::Result<Response> {
    let mut pipe = connect()?;
    let mut line = serde_json::to_vec(request)?;
    line.push(b'\n');
    pipe.write_all(&line)?;
    let mut reply = String::new();
    BufReader::new(pipe).read_line(&mut reply)?;
    serde_json::from_str(&reply).map_err(std::io::Error::other)
}
