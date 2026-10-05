//! Runs fixes one at a time, verifies them, and records what happened.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;

use syscura_core::findings::{FindingStatus, FixAttempt, Risk};
use syscura_core::now_ms;
use syscura_rules::{Engine, render};
use syscura_store::Store;

use crate::{actions, log};

pub enum Job {
    Fix { finding: i64, fix: usize, automatic: bool },
    Undo { attempt: i64 },
}

/// A fix waiting out its "quiet" period before it counts as verified.
struct Pending {
    attempt: i64,
    finding: i64,
    due_ms: i64,
}

pub fn start(db_path: PathBuf, rx: Receiver<Job>) -> Result<(), String> {
    std::thread::Builder::new()
        .name("healer".into())
        .stack_size(512 * 1024)
        .spawn(move || run(db_path, rx))
        .map(|_| ())
        .map_err(|e| format!("cannot start the healer: {e}"))
}

fn run(db_path: PathBuf, rx: Receiver<Job>) {
    let store = match Store::open(&db_path) {
        Ok(s) => s,
        Err(e) => return log::error(&format!("healer cannot open the store: {e}")),
    };
    let engine = match Engine::builtin() {
        Ok(e) => e,
        Err(e) => return log::error(&e),
    };
    let mut pending: Vec<Pending> = Vec::new();
    loop {
        match rx.recv_timeout(Duration::from_secs(30)) {
            Ok(Job::Fix { finding, fix, automatic }) => {
                if let Some(p) = apply(&store, &engine, finding, fix, automatic) {
                    pending.push(p);
                }
            }
            Ok(Job::Undo { attempt }) => undo(&store, attempt),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        // Fixes whose problem stayed away for the whole quiet period worked.
        let now = now_ms();
        pending.retain(|p| {
            if p.due_ms > now {
                return true;
            }
            if let Ok(Some(f)) = store.finding(p.finding)
                && f.status == FindingStatus::Fixed
            {
                let _ = store.set_attempt_verified(p.attempt, true, None);
            }
            false
        });
    }
}

fn apply(store: &Store, engine: &Engine, finding_id: i64, fix: usize, automatic: bool) -> Option<Pending> {
    let finding = store.finding(finding_id).ok()??;
    let rule = engine.rule(&finding.rule_id)?;
    let spec = rule.fixes.get(fix)?;
    let fill = |m: &BTreeMap<String, String>| -> BTreeMap<String, String> {
        m.iter().map(|(k, v)| (k.clone(), render(v, &finding.evidence, finding.count))).collect()
    };
    let before = finding.status;
    let _ = store.set_finding_status(finding_id, FindingStatus::Fixing);
    log::info(&format!("fix {} for finding {finding_id}: {}", spec.action, spec.label));

    let mut attempt = FixAttempt {
        id: 0,
        ts: now_ms(),
        fix_index: fix,
        label: spec.label.clone(),
        automatic,
        ok: false,
        verified: None,
        message: String::new(),
        undo: None,
        undone: false,
    };
    let mut quiet_mins: Option<i64> = None;
    match actions::run(&spec.action, &fill(&spec.params)) {
        Ok(outcome) => {
            attempt.ok = true;
            attempt.message = outcome.message;
            attempt.undo = outcome.undo;
            for v in &rule.verify {
                if v.probe == "quiet" {
                    quiet_mins = v.params.get("mins").and_then(|m| m.parse().ok());
                    continue;
                }
                match actions::probe(&v.probe, &fill(&v.params)) {
                    Ok(true) => {}
                    Ok(false) => {
                        attempt.verified = Some(false);
                        attempt.message.push_str(" Check after the fix failed.");
                    }
                    Err(e) => {
                        attempt.verified = Some(false);
                        attempt.message.push_str(&format!(" Could not check the result: {e}."));
                    }
                }
            }
            if attempt.verified.is_none() && quiet_mins.is_none() {
                attempt.verified = Some(true);
            }
        }
        Err(e) => {
            attempt.verified = Some(false);
            attempt.message = explain_error(&e);
        }
    }

    let attempt_id = store.add_attempt(finding_id, &attempt).ok()?;
    // A restore point prepares for a fix; it does not fix anything itself.
    let status = if spec.action == "restore_point" {
        if before == FindingStatus::Fixing { FindingStatus::Open } else { before }
    } else if attempt.ok && attempt.verified != Some(false) {
        FindingStatus::Fixed
    } else {
        FindingStatus::FixFailed
    };
    let _ = store.set_finding_status(finding_id, status);
    log::info(&format!("fix result for finding {finding_id}: {} ({})", status.as_str(), attempt.message));

    match (status, quiet_mins) {
        (FindingStatus::Fixed, Some(mins)) => Some(Pending { attempt: attempt_id, finding: finding_id, due_ms: now_ms() + mins * 60_000 }),
        _ => None,
    }
}

fn undo(store: &Store, attempt_id: i64) {
    let Ok(Some((finding_id, attempt))) = store.attempt(attempt_id) else { return };
    let Some(token) = attempt.undo.filter(|_| !attempt.undone) else { return };
    match actions::undo(&token) {
        Ok(msg) => {
            let _ = store.mark_undone(attempt_id);
            let _ = store.set_finding_status(finding_id, FindingStatus::Open);
            log::info(&format!("undo for attempt {attempt_id}: {msg}"));
        }
        Err(e) => log::error(&format!("undo for attempt {attempt_id} failed: {e}")),
    }
}

/// Turns raw Windows errors into something a person can act on.
fn explain_error(e: &str) -> String {
    let l = e.to_ascii_lowercase();
    if l.contains("access is denied") || l.contains("os error 5") || l.contains("0x80070005") || l.contains("requires elevation") {
        format!("{e} This needs administrator rights: install the Syscura service so it can apply fixes.")
    } else {
        e.to_string()
    }
}

/// Picks the fix to run automatically for a finding, if any: the first
/// safe, auto-allowed fix that has not already failed twice.
pub fn auto_fix_choice(engine: &Engine, store: &Store, finding_id: i64, rule_id: &str) -> Option<usize> {
    let rule = engine.rule(rule_id)?;
    let now = now_ms();
    // At most 6 automatic fixes per hour across everything.
    if store.automatic_attempts_since(now - 60 * 60 * 1000).ok()? >= 6 {
        return None;
    }
    rule.fixes.iter().enumerate().find_map(|(i, f)| {
        let ok = f.risk == Risk::Safe
            && f.auto
            && f.action != "restore_point"
            && store.failed_attempts(finding_id, i).unwrap_or(0) < 2;
        ok.then_some(i)
    })
}
