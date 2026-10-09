//! "Back up my files": copies the user's important folders to another
//! drive with Windows' own robocopy, in copy-only mode. It never deletes,
//! never mirrors (no /MIR or /PURGE) and never moves files.

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::UI::Shell::{
    FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Downloads, FOLDERID_Music, FOLDERID_Pictures, FOLDERID_Videos,
    KF_FLAG_DEFAULT, SHGetKnownFolderPath,
};
use windows::core::GUID;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Serialize, Clone)]
pub struct Folder {
    pub id: String,
    pub name: String,
    pub path: String,
}

#[derive(Serialize, Clone)]
pub struct Target {
    /// "D:\"
    pub root: String,
    pub label: String,
    pub free_bytes: u64,
    pub size_bytes: u64,
    /// Same drive as Windows: protects against mistakes, not against a dying drive.
    pub system_drive: bool,
    pub removable: bool,
}

#[derive(Serialize, Clone, Default)]
pub struct Status {
    pub running: bool,
    pub destination: String,
    /// Folder being copied now.
    pub current: String,
    pub done: Vec<FolderResult>,
    pub finished_ms: Option<i64>,
    pub error: Option<String>,
}

/// A backup that repeats by itself while Syscura runs in the tray.
#[derive(Serialize, serde::Deserialize, Clone)]
pub struct Schedule {
    pub every_days: u32,
    /// Drive root or folder, e.g. "E:\".
    pub destination: String,
    /// Folder ids, as in `folders()`.
    pub folders: Vec<String>,
    /// Unix ms of the last scheduled run.
    #[serde(default)]
    pub last_ms: i64,
}

impl Schedule {
    pub fn validate(&self) -> Result<(), String> {
        if !(1..=31).contains(&self.every_days) {
            return Err("Choose every 1 to 31 days.".into());
        }
        if self.folders.is_empty() {
            return Err("Choose at least one folder to back up.".into());
        }
        let b = self.destination.as_bytes();
        if b.len() < 3 || !b[0].is_ascii_alphabetic() || b[1] != b':' || b[2] != b'\\' {
            return Err("Choose a drive to back up to.".into());
        }
        Ok(())
    }

    pub fn due(&self, now_ms: i64) -> bool {
        now_ms - self.last_ms >= self.every_days as i64 * 86_400_000
    }
}

#[derive(Serialize, Clone)]
pub struct FolderResult {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

/// The user's real folders (they may have been moved, e.g. into OneDrive).
pub fn folders() -> Vec<Folder> {
    let known: [(&str, &str, GUID); 6] = [
        ("desktop", "Desktop", FOLDERID_Desktop),
        ("documents", "Documents", FOLDERID_Documents),
        ("pictures", "Pictures", FOLDERID_Pictures),
        ("videos", "Videos", FOLDERID_Videos),
        ("music", "Music", FOLDERID_Music),
        ("downloads", "Downloads", FOLDERID_Downloads),
    ];
    known
        .iter()
        .filter_map(|(id, name, guid)| {
            let path = unsafe {
                let p = SHGetKnownFolderPath(guid, KF_FLAG_DEFAULT, None).ok()?;
                let s = p.to_string().ok();
                CoTaskMemFree(Some(p.0 as *const _));
                s?
            };
            Path::new(&path).is_dir().then(|| Folder { id: id.to_string(), name: name.to_string(), path })
        })
        .collect()
}

pub fn targets(volumes: &[syscura_core::hw::VolumeInfo]) -> Vec<Target> {
    let sys = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into()).to_ascii_uppercase();
    let mut out: Vec<Target> = volumes
        .iter()
        .filter(|v| v.size_bytes > 0)
        .map(|v| Target {
            root: format!("{}\\", v.letter),
            label: v.label.clone(),
            free_bytes: v.free_bytes,
            size_bytes: v.size_bytes,
            system_drive: v.letter.eq_ignore_ascii_case(&sys),
            removable: v.removable,
        })
        .collect();
    // Other drives first: they survive if the Windows drive fails.
    out.sort_by_key(|t| (t.system_drive, std::cmp::Reverse(t.free_bytes)));
    out
}

/// Starts copying in the background. `dest_root` must be an existing drive
/// root or folder; a dated "Syscura Backup" folder is created inside it
/// (or `fixed`, for scheduled backups: copying into the same folder again
/// only copies what changed, and still never deletes anything).
pub fn start(status: Arc<Mutex<Status>>, dest_root: String, chosen: Vec<String>, fixed: Option<&str>) -> Result<String, String> {
    if status.lock().unwrap_or_else(|e| e.into_inner()).running {
        return Err("A backup is already running.".into());
    }
    let root = PathBuf::from(dest_root.trim());
    if !root.is_dir() {
        return Err("That destination does not exist.".into());
    }
    let all = folders();
    let picked: Vec<Folder> = all.into_iter().filter(|f| chosen.contains(&f.id)).collect();
    if picked.is_empty() {
        return Err("Choose at least one folder to back up.".into());
    }
    // Never copy a folder into itself.
    let root_lc = root.to_string_lossy().to_ascii_lowercase();
    if let Some(f) = picked.iter().find(|f| root_lc.starts_with(&f.path.to_ascii_lowercase())) {
        return Err(format!("The destination is inside your {} folder. Pick another drive.", f.name));
    }
    let stamp = fixed.map(str::to_string).unwrap_or_else(chrono_like_stamp);
    let dest = root.join("Syscura Backup").join(&stamp);
    std::fs::create_dir_all(&dest).map_err(|e| format!("Cannot create {}: {e}", dest.display()))?;

    *status.lock().unwrap_or_else(|e| e.into_inner()) =
        Status { running: true, destination: dest.to_string_lossy().into_owned(), ..Default::default() };
    let shown = dest.to_string_lossy().into_owned();
    std::thread::spawn(move || {
        let windir = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        let robocopy = Path::new(&windir).join(r"System32\robocopy.exe");
        for f in picked {
            status.lock().unwrap_or_else(|e| e.into_inner()).current = f.name.clone();
            let target = dest.join(&f.name);
            // /E copy subfolders, /XJ skip junction loops, /R:1 /W:1 do not hang on locked files,
            // /MT:8 copy in parallel, /COPY:DAT data+attributes+times. No /MIR, /PURGE or /MOV.
            let result = Command::new(&robocopy)
                .arg(&f.path)
                .arg(&target)
                .args(["/E", "/XJ", "/R:1", "/W:1", "/MT:8", "/COPY:DAT", "/NP", "/NFL", "/NDL", "/NJH"])
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .creation_flags(CREATE_NO_WINDOW)
                .output();
            let entry = match result {
                // robocopy: 0-7 mean success (with or without copying), 8+ some files failed.
                Ok(out) => {
                    let code = out.status.code().unwrap_or(16);
                    let text = String::from_utf8_lossy(&out.stdout);
                    let files = text.lines().find(|l| l.trim_start().starts_with("Files :")).map(|l| l.trim().to_string()).unwrap_or_default();
                    FolderResult {
                        name: f.name.clone(),
                        ok: code < 8,
                        detail: if code < 8 { files } else { format!("Some files could not be copied (they may be open or locked). {files}") },
                    }
                }
                Err(e) => FolderResult { name: f.name.clone(), ok: false, detail: e.to_string() },
            };
            status.lock().unwrap_or_else(|e| e.into_inner()).done.push(entry);
        }
        let mut s = status.lock().unwrap_or_else(|e| e.into_inner());
        s.running = false;
        s.current.clear();
        s.finished_ms = Some(syscura_core::now_ms());
    });
    Ok(shown)
}

/// "2026-10-06 0812" from the local clock, without extra dependencies.
fn chrono_like_stamp() -> String {
    use windows::Win32::System::SystemInformation::GetLocalTime;
    let t = unsafe { GetLocalTime() };
    format!("{:04}-{:02}-{:02} {:02}{:02}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute)
}

#[cfg(test)]
mod tests {
    use super::Schedule;

    #[test]
    fn schedules_are_checked() {
        let s = |days, dest: &str, folders: &[&str]| Schedule {
            every_days: days,
            destination: dest.into(),
            folders: folders.iter().map(|f| f.to_string()).collect(),
            last_ms: 0,
        };
        assert!(s(7, "E:\\", &["documents"]).validate().is_ok());
        assert!(s(0, "E:\\", &["documents"]).validate().is_err());
        assert!(s(7, "E:\\", &[]).validate().is_err());
        assert!(s(7, "nowhere", &["documents"]).validate().is_err());
        let mut w = s(7, "E:\\", &["documents"]);
        assert!(w.due(8 * 86_400_000), "never run: due");
        w.last_ms = 1_000;
        assert!(!w.due(1_000 + 6 * 86_400_000));
        assert!(w.due(1_000 + 7 * 86_400_000));
    }
}
