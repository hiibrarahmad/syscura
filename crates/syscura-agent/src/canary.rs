//! Ransomware tripwire (off until the person turns it on in Settings).
//!
//! Syscura places one small hidden "canary" file in each person's Documents
//! and Pictures folders. Nobody has a reason to open it; ransomware encrypts
//! every file it finds, so when a canary's content changes, or it is
//! renamed with a new extension, files are being encrypted right now. The
//! check reads a few tiny files every 5 seconds: no hooks, no driver.
//!
//! A canary that is simply deleted gets a quieter warning (people clean up
//! hidden files) and is put back.

use std::collections::HashMap;
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::Duration;

use sha2::{Digest, Sha256};
use syscura_core::Level;

use crate::agent::Msg;
use crate::reg::{self, HKEY_LOCAL_MACHINE, HKEY_USERS};
use crate::watch::emit;

pub const OPTION: &str = "ransomware_canary";
const NAME: &str = "!syscura-canary-do-not-open.docx";
const POLL: Duration = Duration::from_secs(5);
const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;

/// The text inside a canary. Random bytes follow, so each one is unique.
const NOTE: &str = "Syscura ransomware tripwire. This hidden file is checked every few seconds: if a program \
changes it, Syscura warns you that files are being encrypted. It is safe to leave alone. Turn it off in \
Syscura -> Settings and it is removed.\r\n";

/// Documents and Pictures of every person with a profile on this PC
/// (following OneDrive or other moved folders for people signed in now).
fn folders() -> Vec<(String, PathBuf)> {
    let list = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\ProfileList";
    let mut out = Vec::new();
    for sid in reg::subkeys(HKEY_LOCAL_MACHINE, list) {
        if !sid.starts_with("S-1-5-21-") {
            continue;
        }
        let Some(profile) = reg::string(HKEY_LOCAL_MACHINE, &format!(r"{list}\{sid}"), "ProfileImagePath") else { continue };
        let profile = PathBuf::from(crate::watch::expand(&profile));
        if !profile.is_dir() {
            continue;
        }
        let user = profile.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let shell = format!(r"{sid}\Software\Microsoft\Windows\CurrentVersion\Explorer\User Shell Folders");
        for (value, default) in [("Personal", "Documents"), ("My Pictures", "Pictures")] {
            let dir = reg::string(HKEY_USERS, &shell, value)
                .map(|d| PathBuf::from(d.replace("%USERPROFILE%", &profile.to_string_lossy())))
                .filter(|d| d.is_absolute() && !d.to_string_lossy().contains('%'))
                .unwrap_or_else(|| profile.join(default));
            if dir.is_dir() {
                out.push((user.clone(), dir));
            }
        }
    }
    out
}

fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn plant(path: &Path) -> Option<[u8; 32]> {
    let mut content = NOTE.as_bytes().to_vec();
    let seed = format!("{}{:?}{}", syscura_core::now_ms(), path, std::process::id());
    for _ in 0..16 {
        content.extend_from_slice(&digest(seed.as_bytes()));
    }
    let _ = std::fs::remove_file(path);
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).attributes(FILE_ATTRIBUTE_HIDDEN).open(path).ok()?;
    std::io::Write::write_all(&mut f, &content).ok()?;
    Some(digest(&content))
}

/// A renamed copy (ransomware often appends its own extension).
fn renamed_copy(dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).find(|p| {
        let n = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        n.len() > NAME.len() && n.starts_with(NAME)
    })
}

pub fn start(db_path: PathBuf, tx: Sender<Msg>) {
    let spawned = std::thread::Builder::new().name("canary".into()).stack_size(256 * 1024).spawn(move || run(&db_path, &tx));
    if let Err(e) = spawned {
        crate::log::error(&format!("cannot start the ransomware tripwire: {e}"));
    }
}

fn enabled(store: &Option<syscura_store::Store>) -> bool {
    store.as_ref().and_then(|s| s.setting(OPTION).ok().flatten()).is_some_and(|v| v == "on")
}

fn run(db_path: &Path, tx: &Sender<Msg>) {
    let mut store = None;
    let mut planted: HashMap<PathBuf, [u8; 32]> = HashMap::new();
    let mut tick = 0u64;
    let mut on = false;
    loop {
        std::thread::sleep(POLL);
        tick += 1;
        if store.is_none() {
            store = syscura_store::Store::open(db_path).ok();
        }
        // The option is read every 30 seconds.
        if tick % 6 == 1 {
            let was = on;
            on = enabled(&store);
            // Turned off (also while the agent was stopped): remove the files.
            if !on && (was || tick == 1) {
                for (_, dir) in folders() {
                    let _ = std::fs::remove_file(dir.join(NAME));
                }
                for path in planted.keys() {
                    let _ = std::fs::remove_file(path);
                }
                if was {
                    crate::log::info("ransomware tripwire off: canary files removed");
                }
                planted.clear();
            }
        }
        if !on {
            continue;
        }
        if planted.is_empty() || tick % 720 == 1 {
            // New people, moved folders: look again every hour.
            let before = planted.len();
            for (_, dir) in folders() {
                let path = dir.join(NAME);
                if !planted.contains_key(&path)
                    && let Some(h) = plant(&path)
                {
                    planted.insert(path, h);
                }
            }
            if planted.len() != before {
                crate::log::info(&format!("ransomware tripwire on: watching {} folders", planted.len()));
            }
        }
        check(&mut planted, tx);
    }
}

fn check(planted: &mut HashMap<PathBuf, [u8; 32]>, tx: &Sender<Msg>) {
    let mut replant = Vec::new();
    for (path, expected) in planted.iter() {
        let dir = path.parent().unwrap_or(Path::new("")).to_path_buf();
        let user = dir.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let minute = syscura_core::now_ms() / 60_000;
        match std::fs::read(path) {
            Ok(bytes) if digest(&bytes) == *expected => continue,
            // Changed in place: encrypted (or someone edited a hidden file).
            Ok(_) => {
                emit(tx, "Syscura/Ransomware", 40, Level::Critical, &format!("changed|{}|{minute}", path.display()), [
                    ("Folder", dir.display().to_string()),
                    ("User", user),
                    ("What", "its content was changed".to_string()),
                ]);
            }
            Err(_) => match renamed_copy(&dir) {
                Some(renamed) => {
                    emit(tx, "Syscura/Ransomware", 40, Level::Critical, &format!("renamed|{}|{minute}", path.display()), [
                        ("Folder", dir.display().to_string()),
                        ("User", user),
                        ("What", format!("it was renamed to {}", renamed.file_name().unwrap_or_default().to_string_lossy())),
                    ]);
                }
                None => {
                    emit(tx, "Syscura/Ransomware", 41, Level::Warning, &format!("gone|{}|{minute}", path.display()), [
                        ("Folder", dir.display().to_string()),
                        ("User", user),
                    ]);
                }
            },
        }
        replant.push(path.clone());
    }
    for path in replant {
        planted.remove(&path);
        if let Some(h) = plant(&path) {
            planted.insert(path, h);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canary_changes_are_caught() {
        let dir = std::env::temp_dir().join(format!("syscura-canary-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(NAME);
        let mut planted = HashMap::from([(path.clone(), plant(&path).unwrap())]);
        let (tx, rx) = std::sync::mpsc::channel();

        check(&mut planted, &tx);
        assert!(rx.try_recv().is_err(), "an untouched canary is quiet");

        // "Encrypt" it.
        std::fs::write(&path, b"\x8f\x02 encrypted garbage").unwrap();
        check(&mut planted, &tx);
        let Ok(Msg::Event(e)) = rx.try_recv() else { panic!("no alarm") };
        assert_eq!((e.event_id, e.level), (40, Level::Critical));

        // Renamed with a ransomware extension.
        std::fs::rename(&path, dir.join(format!("{NAME}.locked"))).unwrap();
        check(&mut planted, &tx);
        let Ok(Msg::Event(e)) = rx.try_recv() else { panic!("no alarm") };
        assert_eq!(e.event_id, 40);
        assert!(e.data["What"].contains(".locked"));
        assert!(path.exists(), "the canary is put back");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn finds_profile_folders() {
        // Every PC has at least one profile with Documents (CI runners too).
        assert!(!folders().is_empty());
    }
}
