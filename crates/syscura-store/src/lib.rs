//! SQLite-backed log of everything Syscura sees and does.
//!
//! The writer (agent's sensor thread) and readers (IPC thread) each open
//! their own `Store`; WAL mode lets them work concurrently.

mod findings;

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};
use syscura_core::{Event, Level, LevelCounts, StoredEvent};

pub use findings::{FindingRow, Recorded};
pub use rusqlite::Error;
pub type Result<T> = rusqlite::Result<T>;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS events (
    id        INTEGER PRIMARY KEY,
    ts        INTEGER NOT NULL,
    source    TEXT    NOT NULL,
    channel   TEXT    NOT NULL,
    provider  TEXT    NOT NULL,
    event_id  INTEGER NOT NULL,
    level     INTEGER NOT NULL,
    record_id INTEGER NOT NULL,
    data      TEXT    NOT NULL,
    UNIQUE (source, channel, record_id)
);
CREATE INDEX IF NOT EXISTS events_ts ON events (ts);
CREATE TABLE IF NOT EXISTS settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS disk_history (
    day    TEXT NOT NULL,
    disk   TEXT NOT NULL,
    health TEXT NOT NULL,
    temp_c REAL,
    wear   INTEGER,
    hours  INTEGER,
    PRIMARY KEY (day, disk)
);
";

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        // Small page cache (256 KiB) keeps the agent's RAM flat; the data
        // set is tiny and mostly append-only, so this costs little speed.
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA cache_size = -256;
             PRAGMA busy_timeout = 2000;",
        )?;
        conn.execute_batch(SCHEMA)?;
        conn.execute_batch(findings::SCHEMA)?;
        findings::migrate(&conn)?;
        Ok(Store { conn })
    }

    /// SQLite's quick integrity check. True when the database is sound.
    pub fn quick_check(&self) -> Result<bool> {
        let verdict: String = self.conn.query_row("PRAGMA quick_check(1)", [], |r| r.get(0))?;
        Ok(verdict.eq_ignore_ascii_case("ok"))
    }

    /// Writes a compact, consistent copy of the database to `dest` (which
    /// must not exist yet). Safe while the agent keeps running.
    pub fn backup_to(&self, dest: &Path) -> Result<()> {
        self.conn.execute("VACUUM INTO ?1", [dest.to_string_lossy()])?;
        Ok(())
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        conn.execute_batch(findings::SCHEMA)?;
        Ok(Store { conn })
    }

    /// Stores an event. Returns false if it was already stored.
    pub fn insert_event(&self, e: &Event) -> Result<bool> {
        let data = serde_json::to_string(&e.data).unwrap_or_else(|_| "{}".into());
        let n = self.conn.execute(
            "INSERT OR IGNORE INTO events
                 (ts, source, channel, provider, event_id, level, record_id, data)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                e.ts,
                e.source,
                e.channel,
                e.provider,
                e.event_id,
                e.level as u8,
                e.record_id as i64,
                data
            ],
        )?;
        Ok(n == 1)
    }

    /// Newest events first, optionally only those at `max_level` or worse.
    pub fn recent_events(&self, limit: u32, max_level: Option<Level>) -> Result<Vec<StoredEvent>> {
        let max = max_level.unwrap_or(Level::Verbose) as u8;
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, ts, source, channel, provider, event_id, level, record_id, data
             FROM events WHERE level <= ?1 ORDER BY ts DESC, id DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![max, limit], |r| {
            let data: String = r.get(8)?;
            Ok(StoredEvent {
                id: r.get(0)?,
                event: Event {
                    ts: r.get(1)?,
                    source: r.get(2)?,
                    channel: r.get(3)?,
                    provider: r.get(4)?,
                    event_id: r.get(5)?,
                    level: Level::from_u8(r.get(6)?),
                    record_id: r.get::<_, i64>(7)? as u64,
                    data: serde_json::from_str(&data).unwrap_or_default(),
                },
            })
        })?;
        rows.collect()
    }

    pub fn count_events(&self) -> Result<u64> {
        self.conn
            .query_row("SELECT COUNT(*) FROM events", [], |r| r.get::<_, i64>(0))
            .map(|n| n as u64)
    }

    /// Per-level counts of events newer than `since_ms`.
    pub fn level_counts_since(&self, since_ms: i64) -> Result<LevelCounts> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT level, COUNT(*) FROM events WHERE ts >= ?1 GROUP BY level",
        )?;
        let mut counts = LevelCounts::default();
        let rows = stmt.query_map([since_ms], |r| Ok((r.get::<_, u8>(0)?, r.get::<_, i64>(1)?)))?;
        for row in rows {
            let (level, n) = row?;
            let n = n as u64;
            match Level::from_u8(level) {
                Level::Critical => counts.critical += n,
                Level::Error => counts.error += n,
                Level::Warning => counts.warning += n,
                _ => counts.other += n,
            }
        }
        Ok(counts)
    }

    /// Deletes events older than `before_ms`. Returns how many were removed.
    pub fn prune_before(&self, before_ms: i64) -> Result<usize> {
        self.conn.execute("DELETE FROM events WHERE ts < ?1", [before_ms])
    }

    pub fn setting(&self, key: &str) -> Result<Option<String>> {
        self.conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0)).optional()
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [key, value],
        )?;
        Ok(())
    }

    pub fn settings(&self) -> Result<std::collections::BTreeMap<String, String>> {
        let mut stmt = self.conn.prepare("SELECT key, value FROM settings")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect()
    }

    /// Records a drive's state for `p.day` (one row per drive per day).
    pub fn add_disk_point(&self, p: &syscura_core::DiskPoint) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO disk_history (day, disk, health, temp_c, wear, hours) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![p.day, p.disk, p.health, p.temperature_c, p.wear_pct, p.power_on_hours.map(|h| h as i64)],
        )?;
        Ok(())
    }

    /// Readings of the last `days` days, oldest first.
    pub fn disk_history(&self, days: u32) -> Result<Vec<syscura_core::DiskPoint>> {
        let mut stmt = self.conn.prepare(
            "SELECT day, disk, health, temp_c, wear, hours FROM disk_history
             WHERE day >= date('now', ?1) ORDER BY day, disk",
        )?;
        let rows = stmt.query_map([format!("-{days} days")], |r| {
            Ok(syscura_core::DiskPoint {
                day: r.get(0)?,
                disk: r.get(1)?,
                health: r.get(2)?,
                temperature_c: r.get(3)?,
                wear_pct: r.get(4)?,
                power_on_hours: r.get::<_, Option<i64>>(5)?.map(|h| h as u64),
            })
        })?;
        rows.collect()
    }

    /// Counts for the summary of the last `days` days.
    pub fn summary(&self, days: u32, security_rules: &[String]) -> Result<syscura_core::Summary> {
        let since = syscura_core::now_ms() - days as i64 * 86_400_000;
        let one = |sql: &str| -> Result<u64> { self.conn.query_row(sql, [since], |r| r.get::<_, i64>(0)).map(|n| n as u64) };
        let new_problems = one("SELECT COUNT(*) FROM findings WHERE first_ts >= ?1")?;
        let fixed_automatically = one(
            "SELECT COUNT(DISTINCT finding_id) FROM fix_attempts WHERE ts >= ?1 AND automatic = 1 AND ok = 1 AND outcome = 'fixed'",
        )?;
        let fixed_by_you = one(
            "SELECT COUNT(DISTINCT finding_id) FROM fix_attempts WHERE ts >= ?1 AND automatic = 0 AND ok = 1 AND outcome IN ('fixed', 'gone')",
        )?;
        let open_problems: u64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM findings WHERE status IN ('open', 'fix_failed')", [], |r| r.get::<_, i64>(0))
            .map(|n| n as u64)?;
        let mut security_problems = 0;
        let mut highlights = Vec::new();
        let mut stmt = self.conn.prepare("SELECT rule_id, grp FROM findings WHERE first_ts >= ?1 ORDER BY first_ts DESC")?;
        for row in stmt.query_map([since], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
            let (rule, group) = row?;
            if security_rules.contains(&rule) {
                security_problems += 1;
            }
            if highlights.len() < 5 {
                highlights.push(if group.is_empty() { rule } else { format!("{rule}: {group}") });
            }
        }
        Ok(syscura_core::Summary {
            days,
            events: self.level_counts_since(since)?,
            new_problems,
            fixed_automatically,
            fixed_by_you,
            open_problems,
            security_problems,
            highlights,
        })
    }

    /// Timestamp of the oldest stored event, if any.
    pub fn oldest_ts(&self) -> Result<Option<i64>> {
        self.conn
            .query_row("SELECT MIN(ts) FROM events", [], |r| r.get(0))
            .optional()
            .map(Option::flatten)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(ts: i64, record_id: u64, level: Level) -> Event {
        let mut data = std::collections::BTreeMap::new();
        data.insert("param1".into(), "Print Spooler".into());
        Event {
            ts,
            source: "eventlog".into(),
            channel: "System".into(),
            provider: "Service Control Manager".into(),
            event_id: 7031,
            level,
            record_id,
            data,
        }
    }

    #[test]
    fn insert_dedupes_and_reads_back() {
        let s = Store::open_in_memory().unwrap();
        assert!(s.insert_event(&ev(10, 1, Level::Error)).unwrap());
        assert!(!s.insert_event(&ev(10, 1, Level::Error)).unwrap());
        assert!(s.insert_event(&ev(20, 2, Level::Warning)).unwrap());

        let all = s.recent_events(10, None).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].event.record_id, 2, "newest first");
        assert_eq!(all[1].event.data["param1"], "Print Spooler");

        let errors = s.recent_events(10, Some(Level::Error)).unwrap();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].event.level, Level::Error);
    }

    #[test]
    fn backup_copies_everything_and_checks_clean() {
        let dir = std::env::temp_dir().join(format!("syscura-store-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let s = Store::open(&dir.join("a.db")).unwrap();
        s.insert_event(&ev(10, 1, Level::Error)).unwrap();
        assert!(s.quick_check().unwrap());
        s.backup_to(&dir.join("b.db")).unwrap();
        let b = Store::open(&dir.join("b.db")).unwrap();
        assert_eq!(b.count_events().unwrap(), 1);
        assert!(b.quick_check().unwrap());
        drop((s, b));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn settings_and_disk_history() {
        let s = Store::open_in_memory().unwrap();
        assert_eq!(s.setting("x").unwrap(), None);
        s.set_setting("x", "on").unwrap();
        s.set_setting("x", "off").unwrap();
        assert_eq!(s.setting("x").unwrap().as_deref(), Some("off"));
        assert_eq!(s.settings().unwrap().len(), 1);
        let today: String = s.conn.query_row("SELECT date('now')", [], |r| r.get(0)).unwrap();
        let p = syscura_core::DiskPoint { day: today, disk: "SSD 1".into(), health: "Healthy".into(), wear_pct: Some(3), ..Default::default() };
        s.add_disk_point(&p).unwrap();
        s.add_disk_point(&p).unwrap();
        let h = s.disk_history(30).unwrap();
        assert_eq!(h.len(), 1, "one reading per drive per day");
        assert_eq!(h[0].wear_pct, Some(3));
    }

    #[test]
    fn summary_counts() {
        let s = Store::open_in_memory().unwrap();
        let now = syscura_core::now_ms();
        s.record_hit("sec.threat", "x", now, &Default::default()).unwrap();
        s.record_hit("svc.crash", "y", now, &Default::default()).unwrap();
        let sum = s.summary(7, &["sec.threat".to_string()]).unwrap();
        assert_eq!((sum.new_problems, sum.security_problems, sum.open_problems), (2, 1, 2));
        assert_eq!(sum.highlights.len(), 2);
    }

    #[test]
    fn counts_and_prune() {
        let s = Store::open_in_memory().unwrap();
        s.insert_event(&ev(10, 1, Level::Critical)).unwrap();
        s.insert_event(&ev(20, 2, Level::Error)).unwrap();
        s.insert_event(&ev(30, 3, Level::Warning)).unwrap();

        let c = s.level_counts_since(15).unwrap();
        assert_eq!((c.critical, c.error, c.warning), (0, 1, 1));

        assert_eq!(s.prune_before(25).unwrap(), 2);
        assert_eq!(s.count_events().unwrap(), 1);
        assert_eq!(s.oldest_ts().unwrap(), Some(30));
    }
}
