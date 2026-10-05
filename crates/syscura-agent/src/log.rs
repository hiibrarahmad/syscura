//! Minimal append-only log for the agent's own messages. Syscura's findings go
//! to SQLite; this file is for diagnosing Syscura itself.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

struct Target {
    file: PathBuf,
    echo: bool,
}

static TARGET: OnceLock<Target> = OnceLock::new();

/// `echo` also prints to stderr (console mode).
pub fn init(data_dir: &Path, echo: bool) {
    let _ = std::fs::create_dir_all(data_dir);
    let _ = TARGET.set(Target { file: data_dir.join("agent.log"), echo });
}

pub fn info(msg: &str) {
    write("INFO", msg);
}

pub fn error(msg: &dyn std::fmt::Display) {
    write("ERROR", &msg.to_string());
}

fn write(level: &str, msg: &str) {
    let line = format!("{} {level} {msg}\n", syscura_core::now_ms());
    match TARGET.get() {
        Some(t) => {
            if t.echo {
                eprint!("{line}");
            }
            // Opened per write: messages are rare and this holds no handle.
            if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&t.file) {
                let _ = f.write_all(line.as_bytes());
            }
        }
        None => eprint!("{line}"),
    }
}
