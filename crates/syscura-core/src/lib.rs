//! Shared types for every Syscura crate: events, severity levels and the
//! agent <-> client IPC protocol.

use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

#[cfg(windows)]
pub mod client;
pub mod findings;
pub mod hw;

/// Named pipe the agent listens on (Windows). One JSON request per line,
/// one JSON response per line.
pub const PIPE_NAME: &str = r"\\.\pipe\syscura";

/// Event severity. The numeric values match Windows Event Log levels so
/// lower means more severe, which keeps "level <= N" filters simple.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Critical = 1,
    Error = 2,
    Warning = 3,
    Info = 4,
    Verbose = 5,
}

impl Level {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => Level::Critical,
            2 => Level::Error,
            3 => Level::Warning,
            5 => Level::Verbose,
            // Windows uses 0 for "LogAlways"; treat it as informational.
            _ => Level::Info,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Level::Critical => "critical",
            Level::Error => "error",
            Level::Warning => "warning",
            Level::Info => "info",
            Level::Verbose => "verbose",
        }
    }
}

/// One observation from a sensor (an Event Log record, a process start, ...).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    /// Unix time in milliseconds.
    pub ts: i64,
    /// Sensor that produced it, e.g. "eventlog".
    pub source: String,
    /// Channel or sub-source, e.g. "System".
    pub channel: String,
    pub provider: String,
    pub event_id: u32,
    pub level: Level,
    /// Source-specific sequence number, used to drop duplicates.
    pub record_id: u64,
    /// Named fields from the event payload.
    pub data: BTreeMap<String, String>,
}

/// An event as read back from the store.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredEvent {
    pub id: i64,
    #[serde(flatten)]
    pub event: Event,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LevelCounts {
    pub critical: u64,
    pub error: u64,
    pub warning: u64,
    pub other: u64,
}

/// Microsoft Defender's state, from Get-MpComputerStatus.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DefenderInfo {
    pub antivirus: bool,
    pub realtime: bool,
    pub tamper_protected: bool,
    /// Days since the virus definitions were updated.
    pub signature_age_days: i64,
    /// ISO dates, empty when never.
    pub signatures_updated: String,
    pub last_quick_scan: String,
    pub last_full_scan: String,
    pub checked_ms: i64,
}

/// One running program, for the Processes page.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ProcessInfo {
    pub pid: u32,
    pub parent: u32,
    pub parent_name: String,
    pub name: String,
    /// Full path of the program file; empty when Windows does not say.
    pub path: String,
    pub started_ms: i64,
    pub memory_bytes: u64,
    /// Share of the whole CPU since the previous listing.
    pub cpu_pct: f32,
    pub threads: u32,
    /// "valid", "unsigned", "invalid", "unknown", or "" (not checked yet).
    pub signature: String,
    pub signer: String,
    /// "system", "program_files", "user" or "other".
    pub location: String,
    /// Why Syscura warns about it; empty when it looks normal.
    pub warning: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusInfo {
    pub version: String,
    pub pid: u32,
    /// Running as the Windows service (not just for this session).
    #[serde(default)]
    pub service: bool,
    /// Microsoft Defender's state, checked every half hour.
    #[serde(default)]
    pub defender: Option<DefenderInfo>,
    pub uptime_secs: u64,
    /// Physical RAM the agent occupies right now.
    pub working_set_bytes: u64,
    /// Memory committed only to the agent (not shared DLL pages).
    pub private_bytes: u64,
    pub sensors: Vec<String>,
    pub events_total: u64,
    pub last_24h: LevelCounts,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Request {
    Status,
    Events {
        limit: u32,
        /// Only return events at this level or more severe.
        max_level: Option<Level>,
    },
    /// The hardware inventory. `refresh` rescans instead of using the
    /// snapshot taken at startup.
    Hardware { refresh: bool },
    /// Problems found, newest first. Closed ones (fixed and verified,
    /// ignored) only when asked.
    Findings { include_closed: bool },
    /// Run fix number `fix` for a finding.
    Fix { finding: i64, fix: usize },
    /// Undo a fix attempt that recorded an undo step.
    Undo { attempt: i64 },
    /// Ignore (or stop ignoring) a finding.
    Ignore { finding: i64, ignore: bool },
    /// The fixed list of actions fixes can use.
    Actions,
    /// Run one action from the catalog, e.g. chosen by the AI. `finding`
    /// attaches it to an existing problem; otherwise a problem titled
    /// `title` is created. Automatic requests may only use safe actions.
    ApplyAction {
        finding: Option<i64>,
        title: String,
        action: String,
        params: BTreeMap<String, String>,
        label: String,
        automatic: bool,
    },
    /// Remember a harm verdict for a problem: harmful "no", "maybe" or
    /// "yes" ("" forgets it), given by "you" or "ai". The AI cannot clear
    /// a security threat; only the person can.
    SetVerdict { finding: i64, harmful: String, by: String },
    /// Every running program, with details.
    Processes,
    /// Check now whether the files of file-based problems still exist,
    /// and close the problems whose files are gone.
    RecheckFiles,
    /// Ask a console-mode agent to exit (used before installing the
    /// service). Refused when running as a service.
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum Response {
    Status(StatusInfo),
    Events(Vec<StoredEvent>),
    Hardware(Box<hw::HardwareInfo>),
    Findings(Vec<findings::Finding>),
    Actions(Vec<findings::ActionInfo>),
    Processes(Vec<ProcessInfo>),
    Done(String),
    Error(String),
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_round_trip() {
        let req = Request::Events { limit: 5, max_level: Some(Level::Error) };
        let line = serde_json::to_string(&req).unwrap();
        assert_eq!(line, r#"{"cmd":"events","limit":5,"max_level":"error"}"#);
        let back: Request = serde_json::from_str(&line).unwrap();
        assert!(matches!(back, Request::Events { limit: 5, max_level: Some(Level::Error) }));
    }

    #[test]
    fn level_ordering_matches_severity() {
        assert!(Level::Critical < Level::Error);
        assert!(Level::Warning <= Level::Warning);
        assert_eq!(Level::from_u8(0), Level::Info);
    }
}
