//! Findings (problems) and the fixes tried on them.

use std::collections::BTreeMap;

use rusqlite::{OptionalExtension, params};
use syscura_core::findings::{FindingStatus, FixAttempt};

use crate::{Result, Store};

pub(crate) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS findings (
    id       INTEGER PRIMARY KEY,
    rule_id  TEXT    NOT NULL,
    grp      TEXT    NOT NULL,
    first_ts INTEGER NOT NULL,
    last_ts  INTEGER NOT NULL,
    count    INTEGER NOT NULL,
    status   TEXT    NOT NULL,
    evidence TEXT    NOT NULL,
    verdict    TEXT  NOT NULL DEFAULT '',
    verdict_by TEXT  NOT NULL DEFAULT '',
    UNIQUE (rule_id, grp)
);
CREATE TABLE IF NOT EXISTS fix_attempts (
    id         INTEGER PRIMARY KEY,
    finding_id INTEGER NOT NULL REFERENCES findings(id),
    ts         INTEGER NOT NULL,
    fix_index  INTEGER NOT NULL,
    label      TEXT    NOT NULL,
    automatic  INTEGER NOT NULL,
    ok         INTEGER NOT NULL,
    verified   INTEGER,
    message    TEXT    NOT NULL,
    undo       TEXT,
    undone     INTEGER NOT NULL DEFAULT 0,
    outcome    TEXT    NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS fix_attempts_finding ON fix_attempts (finding_id);
";

/// Brings databases from older versions up to date.
pub(crate) fn migrate(conn: &rusqlite::Connection) -> Result<()> {
    let has_outcome: bool = conn
        .prepare("SELECT 1 FROM pragma_table_info('fix_attempts') WHERE name = 'outcome'")?
        .exists([])?;
    if !has_outcome {
        conn.execute_batch("ALTER TABLE fix_attempts ADD COLUMN outcome TEXT NOT NULL DEFAULT ''")?;
    }
    let has_findings: bool = conn.prepare("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'findings'")?.exists([])?;
    let has_verdict: bool = conn
        .prepare("SELECT 1 FROM pragma_table_info('findings') WHERE name = 'verdict'")?
        .exists([])?;
    if has_findings && !has_verdict {
        conn.execute_batch(
            "ALTER TABLE findings ADD COLUMN verdict TEXT NOT NULL DEFAULT '';
             ALTER TABLE findings ADD COLUMN verdict_by TEXT NOT NULL DEFAULT '';",
        )?;
    }
    Ok(())
}

/// A finding row without the rule-derived text (the agent adds that).
#[derive(Debug, Clone)]
pub struct FindingRow {
    pub id: i64,
    pub rule_id: String,
    pub group: String,
    pub first_ts: i64,
    pub last_ts: i64,
    pub count: u64,
    pub status: FindingStatus,
    pub evidence: BTreeMap<String, String>,
    /// A remembered harm verdict ("no", "maybe", "yes"), or "" for the
    /// rule's default. Kept when the same problem happens again.
    pub verdict: String,
    /// Who gave the verdict: "you" or "ai".
    pub verdict_by: String,
}

/// What recording a hit did to the finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recorded {
    /// A finding that did not exist before.
    New(i64),
    /// An open finding got another occurrence.
    Repeat(i64),
    /// A finding marked fixed came back before its fix was confirmed: the
    /// fix did not hold.
    Returned(i64),
    /// A finding whose fix was confirmed happened again later. The fix is
    /// known to work, so it can simply be applied again.
    CameBack(i64),
    /// The user ignores this finding; only the counter moved.
    Ignored(i64),
}

impl Store {
    /// Adds one occurrence of (rule, group).
    pub fn record_hit(&self, rule_id: &str, group: &str, ts: i64, evidence: &BTreeMap<String, String>) -> Result<Recorded> {
        let ev = serde_json::to_string(evidence).unwrap_or_else(|_| "{}".into());
        let existing: Option<(i64, String)> = self
            .conn
            .query_row(
                "SELECT id, status FROM findings WHERE rule_id = ?1 AND grp = ?2",
                params![rule_id, group],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        match existing {
            None => {
                self.conn.execute(
                    "INSERT INTO findings (rule_id, grp, first_ts, last_ts, count, status, evidence)
                     VALUES (?1, ?2, ?3, ?3, 1, 'open', ?4)",
                    params![rule_id, group, ts, ev],
                )?;
                Ok(Recorded::New(self.conn.last_insert_rowid()))
            }
            Some((id, status)) => {
                let status = FindingStatus::parse(&status);
                let fix_confirmed = status == FindingStatus::Fixed
                    && self
                        .conn
                        .query_row(
                            "SELECT verified FROM fix_attempts WHERE finding_id = ?1 ORDER BY id DESC LIMIT 1",
                            [id],
                            |r| r.get::<_, Option<bool>>(0),
                        )
                        .optional()?
                        .flatten()
                        == Some(true);
                let new_status = match status {
                    FindingStatus::Fixed if fix_confirmed => FindingStatus::Open,
                    FindingStatus::Fixed => FindingStatus::FixFailed,
                    other => other,
                };
                self.conn.execute(
                    "UPDATE findings SET last_ts = MAX(last_ts, ?2), count = count + 1, status = ?3, evidence = ?4 WHERE id = ?1",
                    params![id, ts, new_status.as_str(), ev],
                )?;
                Ok(match status {
                    FindingStatus::Fixed if fix_confirmed => Recorded::CameBack(id),
                    FindingStatus::Fixed => {
                        // The latest fix did not hold.
                        self.conn.execute(
                            "UPDATE fix_attempts SET verified = 0
                             WHERE id = (SELECT MAX(id) FROM fix_attempts WHERE finding_id = ?1)",
                            [id],
                        )?;
                        Recorded::Returned(id)
                    }
                    FindingStatus::Ignored => Recorded::Ignored(id),
                    _ => Recorded::Repeat(id),
                })
            }
        }
    }

    pub fn set_finding_status(&self, id: i64, status: FindingStatus) -> Result<()> {
        self.conn.execute("UPDATE findings SET status = ?2 WHERE id = ?1", params![id, status.as_str()])?;
        Ok(())
    }

    /// Remembers a harm verdict for a problem ("" forgets it).
    pub fn set_verdict(&self, id: i64, verdict: &str, by: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE findings SET verdict = ?2, verdict_by = ?3 WHERE id = ?1",
            params![id, verdict, if verdict.is_empty() { "" } else { by }],
        )?;
        Ok(())
    }

    pub fn finding(&self, id: i64) -> Result<Option<FindingRow>> {
        self.conn
            .query_row(
                "SELECT id, rule_id, grp, first_ts, last_ts, count, status, evidence, verdict, verdict_by FROM findings WHERE id = ?1",
                [id],
                row_to_finding,
            )
            .optional()
    }

    /// Corrects results saved by older versions, which counted "the repair
    /// tool found nothing wrong" as a fix. Reopens those problems.
    pub fn repair_old_outcomes(&self) -> Result<usize> {
        let n = self.conn.execute(
            "UPDATE fix_attempts SET outcome = 'nothing_found', verified = NULL
             WHERE outcome = '' AND ok = 1 AND (message LIKE '%did not find any integrity violations%'
                OR message LIKE '%No component store corruption detected%' OR message LIKE '%found no problems%')",
            [],
        )?;
        // Nothing is running right after the agent starts, so a problem
        // still marked "fixing" lost its job (the agent was stopped mid-fix).
        self.conn.execute(
            "UPDATE findings SET status = 'open' WHERE status = 'fixing'",
            [],
        )?;
        // Older versions kept the progress lines of long repairs in the message.
        self.conn.execute(
            "UPDATE fix_attempts SET message = 'Windows Resource Protection did not find any integrity violations.'
             WHERE message LIKE '%did not find any integrity violations%' AND message LIKE '%!% complete%' ESCAPE '!'",
            [],
        )?;
        // ...and sometimes only those lines, without the result.
        self.conn.execute(
            "UPDATE fix_attempts SET outcome = 'unclear', verified = NULL,
               message = 'An older Syscura version could not read the result. Run it again to see what it found.'
             WHERE outcome = '' AND ok = 1 AND message LIKE '%!% complete%' ESCAPE '!'",
            [],
        )?;
        self.conn.execute(
            "UPDATE findings SET status = 'open' WHERE status IN ('fixed', 'fixing') AND id IN
               (SELECT finding_id FROM fix_attempts a WHERE a.outcome IN ('nothing_found', 'unclear')
                AND a.id = (SELECT MAX(id) FROM fix_attempts WHERE finding_id = a.finding_id))",
            [],
        )?;
        Ok(n)
    }

    /// Newest first. Without `include_closed`, ignored findings and
    /// problems whose fix was confirmed more than a day ago are left out.
    pub fn findings(&self, include_closed: bool, limit: u32) -> Result<Vec<FindingRow>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, rule_id, grp, first_ts, last_ts, count, status, evidence, verdict, verdict_by FROM findings
             ORDER BY last_ts DESC LIMIT ?1",
        )?;
        let day_ago = syscura_core::now_ms() - 24 * 60 * 60 * 1000;
        let rows: Vec<FindingRow> = stmt.query_map([limit], row_to_finding)?.collect::<Result<_>>()?;
        let mut out = Vec::new();
        for f in rows {
            // Hide a fixed problem only once its fix was confirmed and has
            // stayed fixed for a day (not by the age of the event).
            let fix_confirmed_long_ago = f.status == FindingStatus::Fixed
                && self
                    .conn
                    .query_row(
                        "SELECT ts, verified FROM fix_attempts WHERE finding_id = ?1 ORDER BY id DESC LIMIT 1",
                        [f.id],
                        |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Option<bool>>(1)?)),
                    )
                    .optional()?
                    .is_some_and(|(ts, verified)| verified == Some(true) && ts < day_ago);
            let closed = f.status == FindingStatus::Ignored || fix_confirmed_long_ago;
            if include_closed || !closed {
                out.push(f);
            }
        }
        Ok(out)
    }

    pub fn add_attempt(&self, finding_id: i64, a: &FixAttempt) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO fix_attempts (finding_id, ts, fix_index, label, automatic, ok, verified, message, undo, outcome)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![finding_id, a.ts, a.fix_index as i64, a.label, a.automatic, a.ok, a.verified, a.message, a.undo, a.outcome],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn set_attempt_verified(&self, attempt_id: i64, verified: bool, message: Option<&str>) -> Result<()> {
        match message {
            Some(m) => self.conn.execute(
                "UPDATE fix_attempts SET verified = ?2, message = message || ' ' || ?3 WHERE id = ?1",
                params![attempt_id, verified, m],
            )?,
            None => self.conn.execute("UPDATE fix_attempts SET verified = ?2 WHERE id = ?1", params![attempt_id, verified])?,
        };
        Ok(())
    }

    pub fn mark_undone(&self, attempt_id: i64) -> Result<()> {
        self.conn.execute("UPDATE fix_attempts SET undone = 1 WHERE id = ?1", [attempt_id])?;
        Ok(())
    }

    /// (finding id, attempt) for one attempt.
    pub fn attempt(&self, attempt_id: i64) -> Result<Option<(i64, FixAttempt)>> {
        self.conn
            .query_row(
                "SELECT finding_id, id, ts, fix_index, label, automatic, ok, verified, message, undo, undone, outcome
                 FROM fix_attempts WHERE id = ?1",
                [attempt_id],
                |r| Ok((r.get(0)?, row_to_attempt(r, 1)?)),
            )
            .optional()
    }

    pub fn attempts(&self, finding_id: i64) -> Result<Vec<FixAttempt>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, ts, fix_index, label, automatic, ok, verified, message, undo, undone, outcome
             FROM fix_attempts WHERE finding_id = ?1 ORDER BY id DESC LIMIT 20",
        )?;
        let rows = stmt.query_map([finding_id], |r| row_to_attempt(r, 0))?;
        rows.collect()
    }

    /// Automatic fixes started since `since_ms` (for rate limiting).
    pub fn automatic_attempts_since(&self, since_ms: i64) -> Result<u64> {
        self.conn
            .query_row("SELECT COUNT(*) FROM fix_attempts WHERE automatic = 1 AND ts >= ?1", [since_ms], |r| r.get::<_, i64>(0))
            .map(|n| n as u64)
    }

    /// How many times this fix already failed for this finding.
    pub fn failed_attempts(&self, finding_id: i64, fix_index: usize) -> Result<u64> {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM fix_attempts WHERE finding_id = ?1 AND fix_index = ?2 AND (ok = 0 OR verified = 0)",
                params![finding_id, fix_index as i64],
                |r| r.get::<_, i64>(0),
            )
            .map(|n| n as u64)
    }
}

fn row_to_finding(r: &rusqlite::Row<'_>) -> rusqlite::Result<FindingRow> {
    let ev: String = r.get(7)?;
    Ok(FindingRow {
        id: r.get(0)?,
        rule_id: r.get(1)?,
        group: r.get(2)?,
        first_ts: r.get(3)?,
        last_ts: r.get(4)?,
        count: r.get::<_, i64>(5)? as u64,
        status: FindingStatus::parse(&r.get::<_, String>(6)?),
        evidence: serde_json::from_str(&ev).unwrap_or_default(),
        verdict: r.get(8)?,
        verdict_by: r.get(9)?,
    })
}

fn row_to_attempt(r: &rusqlite::Row<'_>, o: usize) -> rusqlite::Result<FixAttempt> {
    Ok(FixAttempt {
        id: r.get(o)?,
        ts: r.get(o + 1)?,
        fix_index: r.get::<_, i64>(o + 2)? as usize,
        label: r.get(o + 3)?,
        automatic: r.get(o + 4)?,
        ok: r.get(o + 5)?,
        verified: r.get(o + 6)?,
        message: r.get(o + 7)?,
        undo: r.get(o + 8)?,
        undone: r.get(o + 9)?,
        outcome: r.get(o + 10)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attempt(ok: bool) -> FixAttempt {
        FixAttempt { id: 0, ts: 5, fix_index: 0, label: "Start it".into(), automatic: true, ok, verified: None, message: "done".into(), undo: None, undone: false, outcome: "fixed".into() }
    }

    #[test]
    fn old_databases_get_the_outcome_column() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE fix_attempts (id INTEGER PRIMARY KEY, finding_id INTEGER, ts INTEGER, fix_index INTEGER,
             label TEXT, automatic INTEGER, ok INTEGER, verified INTEGER, message TEXT, undo TEXT, undone INTEGER NOT NULL DEFAULT 0);",
        )
        .unwrap();
        migrate(&conn).unwrap();
        migrate(&conn).unwrap(); // idempotent
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM pragma_table_info('fix_attempts') WHERE name = 'outcome'", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn hit_lifecycle() {
        let s = Store::open_in_memory().unwrap();
        let ev = BTreeMap::from([("param1".to_string(), "Spooler".to_string())]);
        let Recorded::New(id) = s.record_hit("svc.crash", "Spooler", 10, &ev).unwrap() else { panic!("expected new") };
        assert_eq!(s.record_hit("svc.crash", "Spooler", 20, &ev).unwrap(), Recorded::Repeat(id));
        assert_eq!(s.finding(id).unwrap().unwrap().count, 2);

        // Fixed, then the problem comes back: the fix did not hold.
        let a = s.add_attempt(id, &attempt(true)).unwrap();
        s.set_finding_status(id, FindingStatus::Fixed).unwrap();
        assert_eq!(s.record_hit("svc.crash", "Spooler", 30, &ev).unwrap(), Recorded::Returned(id));
        assert_eq!(s.finding(id).unwrap().unwrap().status, FindingStatus::FixFailed);
        assert_eq!(s.attempt(a).unwrap().unwrap().1.verified, Some(false));
        assert_eq!(s.failed_attempts(id, 0).unwrap(), 1);

        // Ignored findings only count.
        s.set_finding_status(id, FindingStatus::Ignored).unwrap();
        assert_eq!(s.record_hit("svc.crash", "Spooler", 40, &ev).unwrap(), Recorded::Ignored(id));
        assert!(s.findings(false, 50).unwrap().is_empty());
        assert_eq!(s.findings(true, 50).unwrap().len(), 1);
        assert_eq!(s.automatic_attempts_since(0).unwrap(), 1);
    }

    #[test]
    fn confirmed_fix_then_comes_back_and_verdict_is_kept() {
        let s = Store::open_in_memory().unwrap();
        let ev = BTreeMap::new();
        let Recorded::New(id) = s.record_hit("svc.crash", "Spooler", 10, &ev).unwrap() else { panic!("expected new") };
        let a = s.add_attempt(id, &attempt(true)).unwrap();
        s.set_attempt_verified(a, true, None).unwrap();
        s.set_finding_status(id, FindingStatus::Fixed).unwrap();
        s.set_verdict(id, "no", "you").unwrap();

        // A week later it happens again: known problem, known fix.
        assert_eq!(s.record_hit("svc.crash", "Spooler", 99, &ev).unwrap(), Recorded::CameBack(id));
        let f = s.finding(id).unwrap().unwrap();
        assert_eq!(f.status, FindingStatus::Open);
        assert_eq!((f.verdict.as_str(), f.verdict_by.as_str()), ("no", "you"));
        assert_eq!(s.attempt(a).unwrap().unwrap().1.verified, Some(true), "the fix still counts as one that worked");

        s.set_verdict(id, "", "you").unwrap();
        assert_eq!(s.finding(id).unwrap().unwrap().verdict_by, "");
    }
}
