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
