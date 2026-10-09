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
    /// Fix number `fix` from the finding's rule.
    Fix { finding: i64, fix: usize, automatic: bool },
    /// Any action from the catalog (chosen by the user or the AI).
    Action { finding: i64, action: String, params: BTreeMap<String, String>, label: String, automatic: bool },
    Undo { attempt: i64 },
}

/// A fix waiting out its "quiet" period before it counts as verified.
struct Pending {
    attempt: i64,
    finding: i64,
    due_ms: i64,
}

/// Everything needed to run and check one fix.
struct Plan {
    action: String,
    params: BTreeMap<String, String>,
    label: String,
    probes: Vec<(String, BTreeMap<String, String>)>,
    quiet_mins: Option<i64>,
    fix_index: usize,
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
                if let Some(plan) = rule_plan(&store, &engine, finding, fix)
                    && let Some(p) = apply(&store, finding, plan, automatic)
                {
                    pending.push(p);
                }
            }
            Ok(Job::Action { finding, action, params, label, automatic }) => {
                let plan = action_plan(&store, &engine, finding, action, params, label);
                if let Some(p) = apply(&store, finding, plan, automatic) {
                    pending.push(p);
                }
            }
            Ok(Job::Undo { attempt }) => undo(&store, attempt),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        resolve_gone_files(&store, None);
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

fn rule_plan(store: &Store, engine: &Engine, finding_id: i64, fix: usize) -> Option<Plan> {
    let finding = store.finding(finding_id).ok()??;
    let rule = engine.rule(&finding.rule_id)?;
    let spec = rule.fixes.get(fix)?;
    let fill = |m: &BTreeMap<String, String>| -> BTreeMap<String, String> {
        m.iter().map(|(k, v)| (k.clone(), render(v, &finding.evidence, finding.count))).collect()
    };
    let mut plan = Plan {
        action: spec.action.clone(),
        params: fill(&spec.params),
        label: spec.label.clone(),
        probes: Vec::new(),
        quiet_mins: None,
        fix_index: fix,
    };
    for v in &rule.verify {
        if v.probe == "quiet" {
            plan.quiet_mins = v.params.get("mins").and_then(|m| m.parse().ok());
        } else {
            plan.probes.push((v.probe.clone(), fill(&v.params)));
        }
    }
    Some(plan)
}

/// A catalog action: checked right away when there is something to check,
/// and watched for the rule's quiet period when the finding has a rule.
fn action_plan(
    store: &Store,
    engine: &Engine,
    finding_id: i64,
    action: String,
    params: BTreeMap<String, String>,
    label: String,
) -> Plan {
    let mut probes = Vec::new();
    if matches!(action.as_str(), "service.ensure_running" | "service.restart") {
        probes.push(("service_running".to_string(), params.clone()));
    }
    let quiet_mins = store
        .finding(finding_id)
        .ok()
        .flatten()
        .and_then(|f| engine.rule(&f.rule_id).cloned())
        .and_then(|r| r.verify.iter().find(|v| v.probe == "quiet").and_then(|v| v.params.get("mins")?.parse().ok()));
    // Index usize::MAX marks "not one of the rule's own fixes".
    Plan { action, params, label, probes, quiet_mins, fix_index: usize::MAX }
}

fn apply(store: &Store, finding_id: i64, plan: Plan, automatic: bool) -> Option<Pending> {
    let before = store.finding(finding_id).ok()??.status;
    // The agent, not the caller, decides what may run unattended.
    if automatic && actions::risk(&plan.action) != Some(Risk::Safe) {
        log::error(&format!("refused automatic {}: not a safe action", plan.action));
        return None;
    }
    let _ = store.set_finding_status(finding_id, FindingStatus::Fixing);
    log::info(&format!("fix {} for finding {finding_id}: {}", plan.action, plan.label));

    let mut attempt = FixAttempt {
        id: 0,
        ts: now_ms(),
        fix_index: plan.fix_index,
        label: plan.label.clone(),
        automatic,
        ok: false,
        verified: None,
        message: String::new(),
        undo: None,
        undone: false,
        outcome: String::new(),
    };
    // Changes that are not "safe" get a restore point first (best effort:
    // Windows allows one per day and needs System Protection turned on).
    let mut note = String::new();
    if matches!(actions::risk(&plan.action), Some(Risk::Caution | Risk::Risky)) && plan.action != "restore_point" {
        note = match actions::run("restore_point", &BTreeMap::new()) {
            Ok(_) => " A restore point was created first.".into(),
            Err(_) => " (No new restore point: Windows allows one per day, or System Protection is off.)".into(),
        };
    }
    let mut effect = actions::Effect::Fixed;
    match actions::run(&plan.action, &plan.params) {
        Ok(outcome) => {
            attempt.ok = true;
            attempt.message = outcome.message;
            attempt.undo = outcome.undo;
            effect = outcome.effect;
            for (probe, params) in &plan.probes {
                match actions::probe(probe, params) {
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
            if attempt.verified.is_none() && plan.quiet_mins.is_none() {
                attempt.verified = Some(true);
            }
        }
        Err(e) => {
            attempt.verified = Some(false);
            attempt.message = explain_error(&e);
        }
    }
    attempt.message.push_str(&note);
    attempt.outcome = if !attempt.ok { "failed".into() } else { effect.as_str().into() };
    // "Nothing found" is not a fix: the problem is still there.
    if attempt.ok && effect == actions::Effect::NothingFound {
        attempt.verified = None;
    }

    let attempt_id = store.add_attempt(finding_id, &attempt).ok()?;
    // A restore point prepares for a fix; it does not fix anything itself,
    // and neither does a check that found nothing wrong.
    let status = if plan.action == "restore_point" || (attempt.ok && effect == actions::Effect::NothingFound) {
        if before == FindingStatus::Fixing { FindingStatus::Open } else { before }
    } else if attempt.ok && effect == actions::Effect::NotRepaired {
        FindingStatus::FixFailed
    } else if attempt.ok && attempt.verified != Some(false) {
        FindingStatus::Fixed
    } else {
        FindingStatus::FixFailed
    };
    let _ = store.set_finding_status(finding_id, status);
    log::info(&format!("fix result for finding {finding_id}: {} ({})", status.as_str(), attempt.message));

    match (status, plan.quiet_mins) {
        (FindingStatus::Fixed, Some(mins)) => Some(Pending { attempt: attempt_id, finding: finding_id, due_ms: now_ms() + mins * 60_000 }),
        _ => None,
    }
}

/// Problems about a file (a virus Defender found, a suspicious program)
/// are solved once the file is gone: deleted, quarantined or removed by
/// Defender. Checks every such open problem (or just `only`) and closes
/// those whose files no longer exist. Returns how many it closed.
pub fn resolve_gone_files(store: &Store, only: Option<i64>) -> usize {
    const FILE_RULES: &[&str] = &[
        "sec.threat", "proc.fake_system", "proc.unsigned_user", "sec.startup_unsigned", "sec.service_user", "drv.vulnerable",
        "drv.malicious",
    ];
    let Ok(rows) = store.findings(false, 500) else { return 0 };
    let mut closed = 0;
    for f in rows {
        if only.is_some_and(|id| id != f.id)
            || !FILE_RULES.contains(&f.rule_id.as_str())
            || !matches!(f.status, FindingStatus::Open | FindingStatus::FixFailed)
        {
            continue;
        }
        let files = affected_files(f.evidence.get("Path").map(String::as_str).unwrap_or(""));
        if files.is_empty() || files.iter().any(|p| std::path::Path::new(p).exists()) {
            continue;
        }
        let attempt = FixAttempt {
            id: 0,
            ts: now_ms(),
            fix_index: usize::MAX,
            label: "Checked the file".into(),
            automatic: false,
            ok: true,
            verified: Some(true),
            message: format!(
                "{} no longer exist{} (deleted, or removed by Defender), so this problem is solved.",
                files.join(", "),
                if files.len() == 1 { "s" } else { "" }
            ),
            undo: None,
            undone: false,
            outcome: "gone".into(),
        };
        if store.add_attempt(f.id, &attempt).is_ok() && store.set_finding_status(f.id, FindingStatus::Fixed).is_ok() {
            log::info(&format!("finding {} solved: its file is gone", f.id));
            closed += 1;
        }
    }
    closed
}

/// Full file paths in an evidence "Path" field. Defender writes several,
/// for example `containerfile:_D:\x.iso; file:_D:\x.iso->(Rar)a.exe`.
pub fn affected_files(raw: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for part in raw.split(';') {
        let part = part.trim();
        let part = part.split_once(":_").map_or(part, |(_, rest)| rest);
        let part = part.split("->").next().unwrap_or("").trim();
        let b = part.as_bytes();
        let full = b.len() > 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'\\';
        if full && !part.ends_with('\\') && !out.iter().any(|o| o.eq_ignore_ascii_case(part)) {
            out.push(part.to_string());
        }
    }
    out
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

#[cfg(test)]
mod tests {
    use super::affected_files;

    #[test]
    fn reads_defender_paths() {
        let raw = r"containerfile:_D:\a b\Setup.iso; file:_D:\a b\Setup.iso->(UDF)Setup.exe; file:_D:\a b\Setup.iso";
        assert_eq!(affected_files(raw), vec![r"D:\a b\Setup.iso".to_string()]);
        assert_eq!(affected_files(r"C:\Users\x\AppData\Local\Temp\svchost.exe"), vec![r"C:\Users\x\AppData\Local\Temp\svchost.exe".to_string()]);
        assert!(affected_files("").is_empty());
    }
}
