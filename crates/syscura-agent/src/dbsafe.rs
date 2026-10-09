//! Keeps Syscura's database healthy: an integrity check at every start, a
//! daily backup (the last three are kept), and recovery on its own when the
//! file is damaged (a power cut mid-write, a failing drive, a bad sector).
//!
//! A damaged database is never deleted: it is renamed to
//! `syscura.db.damaged-<time>` so it can still be inspected.

use std::path::{Path, PathBuf};

use syscura_store::Store;

use crate::log;

const KEEP_BACKUPS: usize = 3;

fn backup_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("backups")
}

/// Opens the database, checking it first. On damage: moves it aside and
/// continues from the newest sound backup, or from a fresh database.
pub fn open(data_dir: &Path, db_path: &Path) -> Result<Store, String> {
    match Store::open(db_path).and_then(|s| s.quick_check().map(|ok| (s, ok))) {
        Ok((store, true)) => return Ok(store),
        Ok((_, false)) => log::error(&"the database failed its integrity check; recovering"),
        Err(e) => log::error(&format!("cannot open the database ({e}); recovering")),
    }
    let stamp = syscura_core::now_ms();
    for suffix in ["", "-wal", "-shm"] {
        let from = PathBuf::from(format!("{}{suffix}", db_path.display()));
        if from.exists() {
            let to = PathBuf::from(format!("{}.damaged-{stamp}{suffix}", db_path.display()));
            if let Err(e) = std::fs::rename(&from, &to) {
                return Err(format!("the database is damaged and could not be moved aside: {e}"));
            }
        }
    }
    for backup in backups(data_dir) {
        let sound = Store::open(&backup).and_then(|s| s.quick_check()).unwrap_or(false);
        if sound && std::fs::copy(&backup, db_path).is_ok() {
            log::info(&format!("restored the database from {}", backup.display()));
            return Store::open(db_path).map_err(|e| format!("cannot open the restored database: {e}"));
        }
    }
    log::info("no sound backup found; starting a new database");
    Store::open(db_path).map_err(|e| format!("cannot create {}: {e}", db_path.display()))
}

/// Backups, newest first.
fn backups(data_dir: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(backup_dir(data_dir)) else { return Vec::new() };
    let mut files: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("syscura-")) && p.extension().is_some_and(|x| x == "db"))
        .collect();
    // Names carry the date (syscura-YYYYMMDD.db), so they sort by age.
    files.sort();
    files.reverse();
    files
}

/// Makes today's backup if there is none yet, and keeps only the newest few.
pub fn daily_backup(store: &Store, data_dir: &Path) {
    let dir = backup_dir(data_dir);
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let dest = dir.join(format!("syscura-{}.db", crate::disks::today().replace('-', "")));
    if dest.exists() {
        return;
    }
    match store.quick_check() {
        // Never back up a damaged database over good backups.
        Ok(true) => {}
        _ => return log::error(&"skipped today's database backup: the integrity check failed"),
    }
    match store.backup_to(&dest) {
        Ok(()) => log::info(&format!("backed up the database to {}", dest.display())),
        Err(e) => {
            let _ = std::fs::remove_file(&dest);
            return log::error(&format!("database backup failed: {e}"));
        }
    }
    for old in backups(data_dir).into_iter().skip(KEEP_BACKUPS) {
        let _ = std::fs::remove_file(old);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("syscura-dbsafe-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn damaged_database_is_set_aside_and_restored_from_backup() {
        let dir = temp("restore");
        let db = dir.join("syscura.db");
        {
            let s = open(&dir, &db).unwrap();
            s.record_hit("svc.crash", "Spooler", 1, &Default::default()).unwrap();
            daily_backup(&s, &dir);
        }
        assert_eq!(backups(&dir).len(), 1);
        // Garbage over the header: SQLite no longer recognises the file.
        std::fs::write(&db, vec![0x55u8; 8192]).unwrap();
        let _ = std::fs::remove_file(dir.join("syscura.db-wal"));
        let s = open(&dir, &db).unwrap();
        assert_eq!(s.findings(true, 10).unwrap().len(), 1, "the backup's problem is back");
        let damaged = std::fs::read_dir(&dir).unwrap().flatten().any(|e| e.file_name().to_string_lossy().contains(".damaged-"));
        assert!(damaged, "the damaged file is kept, not deleted");
        drop(s);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn damaged_database_without_backup_starts_fresh() {
        let dir = temp("fresh");
        let db = dir.join("syscura.db");
        std::fs::write(&db, b"this is not a database at all, not even close, really").unwrap();
        let s = open(&dir, &db).unwrap();
        assert_eq!(s.count_events().unwrap(), 0);
        drop(s);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
