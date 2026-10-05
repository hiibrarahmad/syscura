//! Hardware inventory for the agent: scanned in a child process, cached in
//! memory, and compared with the previous boot to spot added, removed or
//! swapped parts.

use std::collections::BTreeMap;
use std::io::Read;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use syscura_core::hw::{HardwareInfo, diff_fingerprints};
use syscura_core::{Event, Level, now_ms};

use crate::agent::Msg;
use crate::log;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const SCAN_TIMEOUT: Duration = Duration::from_secs(60);
/// Let Windows finish booting before the first scan.
const STARTUP_DELAY: Duration = Duration::from_secs(20);

#[derive(Default)]
pub struct HardwareCache {
    latest: Mutex<Option<HardwareInfo>>,
}

impl HardwareCache {
    /// Cached inventory, or a fresh scan when asked or when none exists.
    pub fn get(&self, refresh: bool) -> Result<HardwareInfo, String> {
        if !refresh
            && let Some(hw) = self.latest.lock().unwrap_or_else(|e| e.into_inner()).clone()
        {
            return Ok(hw);
        }
        let hw = scan()?;
        *self.latest.lock().unwrap_or_else(|e| e.into_inner()) = Some(hw.clone());
        Ok(hw)
    }
}

/// Runs `syscura-agent hwscan` and parses its JSON output.
pub fn scan() -> Result<HardwareInfo, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut child = Command::new(exe)
        .arg("hwscan")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| format!("cannot start hardware scan: {e}"))?;
    let mut stdout = child.stdout.take().ok_or("no scan output")?;
    // Read on a helper thread so a hung scan can be killed.
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stdout.read_to_string(&mut s);
        s
    });
    let start = Instant::now();
    loop {
        if child.try_wait().map_err(|e| e.to_string())?.is_some() {
            break;
        }
        if start.elapsed() > SCAN_TIMEOUT {
            let _ = child.kill();
            return Err("hardware scan timed out".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let out = reader.join().unwrap_or_default();
    serde_json::from_str(out.trim()).map_err(|e| format!("bad hardware scan output: {e}"))
}

/// Startup task: scan, compare with the saved snapshot, report changes as
/// events, save the new snapshot.
pub fn start_boot_check(data_dir: PathBuf, cache: std::sync::Arc<HardwareCache>, tx: Sender<Msg>) {
    let spawned = std::thread::Builder::new()
        .name("hwcheck".into())
        .stack_size(256 * 1024)
        .spawn(move || {
            std::thread::sleep(STARTUP_DELAY);
            match cache.get(true) {
                Ok(hw) => compare_and_save(&data_dir.join("hardware.json"), &hw, &tx),
                Err(e) => log::error(&format!("hardware check failed: {e}")),
            }
        });
    if let Err(e) = spawned {
        log::error(&format!("cannot start hardware check: {e}"));
    }
}

fn compare_and_save(path: &Path, hw: &HardwareInfo, tx: &Sender<Msg>) {
    let new = hw.fingerprint();
    let old: Option<Vec<String>> = std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<HardwareInfo>(&s).ok())
        .map(|h| h.fingerprint());

    let base = now_ms();
    let mut seq = 0u64;
    let mut emit = |event_id: u32, level: Level, what: &str, part: &str| {
        let mut data = BTreeMap::new();
        data.insert("change".to_string(), what.to_string());
        data.insert("part".to_string(), part.to_string());
        let _ = tx.send(Msg::Event(Event {
            ts: base,
            source: "hardware".into(),
            channel: "inventory".into(),
            provider: "Syscura".into(),
            event_id,
            level,
            record_id: base as u64 * 100 + seq,
            data,
        }));
        seq += 1;
    };

    match old {
        None => emit(3, Level::Info, "first inventory", &format!("{} parts recorded", new.len())),
        Some(old) => {
            let (removed, added) = diff_fingerprints(&old, &new);
            for part in &removed {
                emit(1, Level::Warning, "removed or changed", part);
            }
            for part in &added {
                emit(2, Level::Info, "added", part);
            }
            if !removed.is_empty() || !added.is_empty() {
                log::info(&format!("hardware changed: {} removed, {} added", removed.len(), added.len()));
            }
        }
    }
    match serde_json::to_string_pretty(hw) {
        Ok(json) => {
            if let Err(e) = std::fs::write(path, json) {
                log::error(&format!("cannot save hardware snapshot: {e}"));
            }
        }
        Err(e) => log::error(&format!("cannot encode hardware snapshot: {e}")),
    }
}
