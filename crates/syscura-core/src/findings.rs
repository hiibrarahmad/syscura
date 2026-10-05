//! Problems Syscura found, the fixes it can offer, and what happened when
//! a fix was tried. Shared by the agent, the CLI and the UI.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::Level;

/// How much a fix could disturb the system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Risk {
    /// Cannot break anything (e.g. start a stopped service). Runs automatically.
    Safe,
    /// Changes something, with an undo or a restore point. Asks first.
    Caution,
    /// Could affect how the PC works. Always asks, with a warning.
    Risky,
}

/// Is the problem itself harmful?
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Harm {
    No,
    Maybe,
    Yes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingStatus {
    Open,
    /// A fix is running right now.
    Fixing,
    /// A fix ran; Syscura is watching whether the problem comes back.
    Fixed,
    /// The last fix failed or the problem came back.
    FixFailed,
    /// The user chose to ignore it.
    Ignored,
}

impl FindingStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            FindingStatus::Open => "open",
            FindingStatus::Fixing => "fixing",
            FindingStatus::Fixed => "fixed",
            FindingStatus::FixFailed => "fix_failed",
            FindingStatus::Ignored => "ignored",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "fixing" => FindingStatus::Fixing,
            "fixed" => FindingStatus::Fixed,
            "fix_failed" => FindingStatus::FixFailed,
            "ignored" => FindingStatus::Ignored,
            _ => FindingStatus::Open,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixOption {
    pub index: usize,
    pub label: String,
    pub action: String,
    pub risk: Risk,
    /// Needs administrator rights (the Syscura service has them).
    pub needs_admin: bool,
    /// The fix can be undone.
    pub undoable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixAttempt {
    pub id: i64,
    pub ts: i64,
    pub fix_index: usize,
    pub label: String,
    /// Started automatically (safe fix) rather than by the user.
    pub automatic: bool,
    pub ok: bool,
    /// None while still being verified.
    pub verified: Option<bool>,
    pub message: String,
    /// Present when the change can be undone.
    pub undo: Option<String>,
    pub undone: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub id: i64,
    pub rule_id: String,
    /// What the finding is about (service name, app name, ...).
    pub group: String,
    pub title: String,
    /// "software", "hardware", "security", "network", "storage", "noise".
    pub category: String,
    pub severity: Level,
    pub harmful: Harm,
    pub explanation: String,
    pub advice: String,
    pub first_ts: i64,
    pub last_ts: i64,
    pub count: u64,
    pub status: FindingStatus,
    /// Facts from the most recent matching event.
    pub evidence: BTreeMap<String, String>,
    pub fixes: Vec<FixOption>,
    pub attempts: Vec<FixAttempt>,
}
