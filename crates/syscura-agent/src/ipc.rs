//! Local named-pipe server: one JSON request line in, one JSON response
//! line out, per connection. Served by a single sleeping thread.
//!
//! Security:
//! - `PIPE_REJECT_REMOTE_CLIENTS`: no network access.
//! - DACL: SYSTEM, Administrators and the pipe's owner (the agent's own
//!   account, so it can open spare instances) get full access; other
//!   interactive users may only read and write data. They cannot create pipe
//!   instances, so another process cannot impersonate the agent.
//! - `FILE_FLAG_FIRST_PIPE_INSTANCE` plus always keeping one spare instance
//!   open means nobody else can claim the name between connections either.
//! - Anyone signed in may read (status, problems, events). Changing anything
//!   (fixes, undo, ignore, verdicts, actions) is only accepted from Syscura's
//!   own app or command line next to this agent, or from an elevated
//!   process: otherwise any program could ask the SYSTEM service to mark
//!   itself safe or disable another program's service.

use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::windows::io::FromRawHandle;
use std::path::PathBuf;
use std::sync::Arc;

use syscura_core::findings::{Finding, FindingStatus, FixOption, Harm};
use syscura_core::{Level, PIPE_NAME, Request, Response, StatusInfo, now_ms};
use syscura_rules::{Engine, render};
use syscura_store::Store;
use windows::Win32::Foundation::{ERROR_PIPE_CONNECTED, HANDLE, INVALID_HANDLE_VALUE};
use windows::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows::Win32::Storage::FileSystem::{
    FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAGS_AND_ATTRIBUTES, FlushFileBuffers, PIPE_ACCESS_DUPLEX,
};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, GetNamedPipeClientProcessId, PIPE_READMODE_BYTE,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows::core::HSTRING;

use crate::agent::Shared;
use crate::heal::Job;
use crate::{actions, log, meminfo};

/// OW = owner rights. 0x12008b = FILE_GENERIC_READ | FILE_WRITE_DATA, which
/// leaves out FILE_CREATE_PIPE_INSTANCE.
const SDDL: &str = "D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GA;;;OW)(A;;0x12008b;;;IU)";
const MAX_REQUEST: u64 = 64 * 1024;
const MAX_EVENTS: u32 = 1000;

struct PipeSecurity {
    sa: SECURITY_ATTRIBUTES,
}

// The descriptor is created once and only read afterwards.
unsafe impl Send for PipeSecurity {}

impl PipeSecurity {
    fn new() -> Result<Self, String> {
        let mut sd = PSECURITY_DESCRIPTOR::default();
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                &HSTRING::from(SDDL),
                SDDL_REVISION_1,
                &mut sd,
                None,
            )
        }
        .map_err(|e| format!("cannot build pipe security descriptor: {e}"))?;
        // Intentionally never freed: it lives as long as the process.
        Ok(PipeSecurity {
            sa: SECURITY_ATTRIBUTES {
                nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: sd.0,
                bInheritHandle: false.into(),
            },
        })
    }

    fn create_instance(&self, first: bool) -> Result<HANDLE, String> {
        let mut open_mode = PIPE_ACCESS_DUPLEX;
        if first {
            open_mode |= FILE_FLAG_FIRST_PIPE_INSTANCE;
        }
        let h = unsafe {
            CreateNamedPipeW(
                &HSTRING::from(PIPE_NAME),
                FILE_FLAGS_AND_ATTRIBUTES(open_mode.0),
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                4,
                64 * 1024,
                64 * 1024,
                0,
                Some(&self.sa),
            )
        };
        if h == INVALID_HANDLE_VALUE {
            let err = windows::core::Error::from_thread();
            return Err(if first {
                format!("cannot create {PIPE_NAME} (is another Syscura agent running?): {err}")
            } else {
                format!("cannot create pipe instance: {err}")
            });
        }
        Ok(h)
    }
}

/// Claims the pipe name now (so startup fails fast if it is taken) and
/// serves requests on a background thread.
pub fn start(db_path: PathBuf, shared: Arc<Shared>) -> Result<(), String> {
    let security = PipeSecurity::new()?;
    // The service waits a little for a console agent that is still exiting
    // (installing the service asks it to step aside); a console agent gives
    // way at once, because then the service already has the pipe.
    let attempts = if shared.console { 1 } else { 20 };
    let mut first = security.create_instance(true);
    for _ in 1..attempts {
        if first.is_ok() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
        first = security.create_instance(true);
    }
    let first = first?;
    let first = first.0 as usize; // HANDLE is not Send; pass the raw value.
    std::thread::Builder::new()
        .name("ipc".into())
        .stack_size(256 * 1024)
        .spawn(move || serve(security, HANDLE(first as _), db_path, shared))
        .map_err(|e| format!("cannot start IPC thread: {e}"))?;
    Ok(())
}

fn serve(security: PipeSecurity, mut current: HANDLE, db_path: PathBuf, shared: Arc<Shared>) {
    // Opened lazily: the main thread has already created the schema.
    let mut store: Option<Store> = None;
    let engine = Engine::builtin().ok();
    loop {
        let connected = match unsafe { ConnectNamedPipe(current, None) } {
            Ok(()) => true,
            Err(e) => e.code() == ERROR_PIPE_CONNECTED.to_hresult(),
        };
        // Open the next instance before serving, so the name is never free.
        let next = security.create_instance(false);
        // Takes ownership: the handle is closed when `pipe` drops.
        let pipe = unsafe { File::from_raw_handle(current.0) };
        if connected {
            if store.is_none() {
                match Store::open(&db_path) {
                    Ok(s) => store = Some(s),
                    Err(e) => log::error(&format!("IPC cannot open store: {e}")),
                }
            }
            let trusted = client_is_trusted(current);
            let shutdown = match handle_client(&pipe, store.as_ref(), engine.as_ref(), &shared, trusted) {
                Ok(s) => s,
                Err(e) => {
                    log::error(&format!("IPC client error: {e}"));
                    false
                }
            };
            unsafe {
                let _ = FlushFileBuffers(current);
                let _ = DisconnectNamedPipe(current);
            }
            if shutdown {
                log::info("stopping on request (console mode)");
                std::process::exit(0);
            }
        }
        drop(pipe);
        current = match next {
            Ok(h) => h,
            Err(e) => {
                log::error(&e);
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(1));
                    if let Ok(h) = security.create_instance(false) {
                        break h;
                    }
                }
            }
        };
    }
}

/// Returns true when the agent should exit after this reply.
fn handle_client(pipe: &File, store: Option<&Store>, engine: Option<&Engine>, shared: &Shared, trusted: bool) -> std::io::Result<bool> {
    let mut line = String::new();
    BufReader::new(pipe.take(MAX_REQUEST)).read_line(&mut line)?;
    let mut shutdown = false;
    let response = match serde_json::from_str::<Request>(line.trim()) {
        Ok(req) if changes_something(&req) && !trusted => {
            log::info(&format!("refused a {} request from a program that is not Syscura", request_name(&req)));
            Response::Error(
                "Only the Syscura app can change things. Open Syscura from the Start menu (or run this as administrator).".into(),
            )
        }
        Ok(Request::Shutdown) if shared.console => {
            shutdown = true;
            Response::Done("Stopping.".into())
        }
        Ok(Request::Shutdown) => Response::Error("The Syscura service is stopped from Windows' Services panel.".into()),
        Ok(req) => match (store, engine) {
            (Some(store), Some(engine)) => answer(req, store, engine, shared),
            _ => Response::Error("event store is unavailable".into()),
        },
        Err(e) => Response::Error(format!("bad request: {e}")),
    };
    let mut out = serde_json::to_vec(&response).unwrap_or_default();
    out.push(b'\n');
    let mut writer = pipe;
    writer.write_all(&out)?;
    Ok(shutdown)
}

/// Requests that change the PC or Syscura's memory of it.
fn changes_something(req: &Request) -> bool {
    !matches!(
        req,
        Request::Status
            | Request::Events { .. }
            | Request::Hardware { .. }
            | Request::Findings { .. }
            | Request::Actions
            | Request::Processes
            | Request::RecheckFiles
            | Request::Security { .. }
            | Request::Options
            | Request::DiskHistory
            | Request::Summary { .. }
    )
}

fn request_name(req: &Request) -> String {
    serde_json::to_value(req)
        .ok()
        .and_then(|v| v.get("cmd").and_then(|c| c.as_str()).map(str::to_string))
        .unwrap_or_default()
}

/// Is the program on the other end of the pipe Syscura itself (its app,
/// command line or agent, in this agent's own folder) or elevated?
fn client_is_trusted(pipe: HANDLE) -> bool {
    let mut pid = 0u32;
    if unsafe { GetNamedPipeClientProcessId(pipe, &mut pid) }.is_err() || pid == 0 {
        return false;
    }
    if pid == std::process::id() {
        return true;
    }
    let Some(info) = crate::caller::inspect(pid) else { return false };
    info.elevated || crate::caller::is_own_program(&info.path)
}

fn answer(req: Request, store: &Store, engine: &Engine, shared: &Shared) -> Response {
    let result = match req {
        Request::Status => status(store, shared).map(Response::Status),
        Request::Events { limit, max_level } => store
            .recent_events(limit.min(MAX_EVENTS), max_level)
            .map(Response::Events),
        Request::Hardware { refresh } => {
            return match shared.hardware.get(refresh) {
                Ok(hw) => Response::Hardware(Box::new(hw)),
                Err(e) => Response::Error(e),
            };
        }
        Request::Findings { include_closed } => store.findings(include_closed, 200).map(|rows| {
            Response::Findings(rows.into_iter().filter_map(|r| finding_view(engine, store, r)).collect())
        }),
        Request::Fix { finding, fix } => return start_fix(store, engine, shared, finding, fix),
        Request::Undo { attempt } => {
            return match store.attempt(attempt) {
                Ok(Some((_, a))) if a.undo.is_some() && !a.undone => send(shared, Job::Undo { attempt }, "Undoing the change."),
                Ok(_) => Response::Error("That fix has nothing to undo.".into()),
                Err(e) => Response::Error(format!("store error: {e}")),
            };
        }
        Request::Ignore { finding, ignore } => store
            .set_finding_status(finding, if ignore { FindingStatus::Ignored } else { FindingStatus::Open })
            .map(|_| Response::Done(if ignore { "Ignored." } else { "No longer ignored." }.into())),
        Request::Actions => return Response::Actions(actions::catalog()),
        Request::SetVerdict { finding, harmful, by } => return set_verdict(engine, store, finding, &harmful, &by),
        Request::Processes => return Response::Processes(shared.watch.processes()),
        Request::RecheckFiles => {
            let n = crate::heal::resolve_gone_files(store, None);
            return Response::Done(match n {
                0 => "Checked: the files are still there.".into(),
                1 => "Checked: the file is gone, so the problem is solved.".into(),
                n => format!("Checked: {n} problems are solved because their files are gone."),
            });
        }
        Request::ApplyAction { finding, title, action, params, label, automatic } => {
            return apply_action(store, shared, finding, &title, action, params, label, automatic);
        }
        Request::Security { refresh } => return Response::Security(shared.watch.security(refresh)),
        Request::Options => store.settings().map(Response::Options),
        Request::SetOption { key, value } => return set_option(store, &key, &value),
        Request::DiskHistory => store.disk_history(365).map(Response::DiskHistory),
        Request::Summary { days } => {
            let security: Vec<String> = engine.rules().iter().filter(|r| r.category == "security").map(|r| r.id.clone()).collect();
            store.summary(days.clamp(1, 90), &security).map(Response::Summary)
        }
        Request::Shutdown => unreachable!("handled before"),
    };
    result.unwrap_or_else(|e| Response::Error(format!("store error: {e}")))
}

fn send(shared: &Shared, job: Job, ok: &str) -> Response {
    match shared.jobs.lock().unwrap_or_else(|e| e.into_inner()).send(job) {
        Ok(()) => Response::Done(ok.into()),
        Err(_) => Response::Error("The fixer is not running.".into()),
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_action(
    store: &Store,
    shared: &Shared,
    finding: Option<i64>,
    title: &str,
    action: String,
    params: std::collections::BTreeMap<String, String>,
    label: String,
    automatic: bool,
) -> Response {
    if let Err(e) = actions::validate(&action, &params) {
        return Response::Error(e);
    }
    // Only safe actions may run without a person approving them.
    if automatic && actions::risk(&action) != Some(syscura_core::findings::Risk::Safe) {
        return Response::Error("This fix changes your PC, so it needs your approval.".into());
    }
    let finding = match finding {
        Some(id) => match store.finding(id) {
            Ok(Some(f)) if f.status == FindingStatus::Fixing => {
                return Response::Error("A fix for this problem is already running.".into());
            }
            Ok(Some(_)) => id,
            Ok(None) => return Response::Error("No such problem.".into()),
            Err(e) => return Response::Error(format!("store error: {e}")),
        },
        // A fix for something seen on the Events page: record it as a problem.
        None => {
            let title: String = title.chars().take(120).collect();
            let evidence = std::collections::BTreeMap::from([("title".to_string(), title.clone())]);
            match store.record_hit(AI_RULE, &title, now_ms(), &evidence) {
                Ok(r) => match r {
                    syscura_store::Recorded::New(id)
                    | syscura_store::Recorded::Repeat(id)
                    | syscura_store::Recorded::Returned(id)
                    | syscura_store::Recorded::CameBack(id)
                    | syscura_store::Recorded::Ignored(id) => id,
                },
                Err(e) => return Response::Error(format!("store error: {e}")),
            }
        }
    };
    let label = if label.trim().is_empty() { action.clone() } else { label.chars().take(160).collect() };
    send(shared, Job::Action { finding, action, params, label: label.clone(), automatic }, &format!("Started: {label}"))
}

/// Options people can change, with their allowed values.
const OPTIONS: &[(&str, &[&str])] = &[(crate::canary::OPTION, &["on", "off"])];

fn set_option(store: &Store, key: &str, value: &str) -> Response {
    let Some((_, allowed)) = OPTIONS.iter().find(|(k, _)| *k == key) else {
        return Response::Error(format!("unknown option {key}"));
    };
    if !allowed.contains(&value) {
        return Response::Error(format!("{key} can only be {}", allowed.join(" or ")));
    }
    match store.set_setting(key, value) {
        Ok(()) => {
            log::info(&format!("option {key} = {value}"));
            Response::Done("Saved.".into())
        }
        Err(e) => Response::Error(format!("store error: {e}")),
    }
}

/// Rule id for problems created from AI help on the Events page.
const AI_RULE: &str = "ai";

fn start_fix(store: &Store, engine: &Engine, shared: &Shared, finding: i64, fix: usize) -> Response {
    let row = match store.finding(finding) {
        Ok(Some(r)) => r,
        Ok(None) => return Response::Error("No such problem.".into()),
        Err(e) => return Response::Error(format!("store error: {e}")),
    };
    if row.status == FindingStatus::Fixing {
        return Response::Error("A fix for this problem is already running.".into());
    }
    let Some(spec) = engine.rule(&row.rule_id).and_then(|r| r.fixes.get(fix)) else {
        return Response::Error("No such fix.".into());
    };
    let label = spec.label.clone();
    send(shared, Job::Fix { finding, fix, automatic: false }, &format!("Started: {label}"))
}

/// A stored finding plus the rule's words and fixes.
fn finding_view(engine: &Engine, store: &Store, r: syscura_store::FindingRow) -> Option<Finding> {
    if r.rule_id == AI_RULE {
        return Some(Finding {
            id: r.id,
            rule_id: r.rule_id.clone(),
            group: String::new(),
            title: r.group.clone(),
            category: "software".into(),
            severity: Level::Warning,
            harmful: Harm::Maybe,
            explanation: "A fix you started from the Events page with AI help.".into(),
            advice: String::new(),
            message: String::new(),
            first_ts: r.first_ts,
            last_ts: r.last_ts,
            count: r.count,
            status: r.status,
            fixes: Vec::new(),
            attempts: store.attempts(r.id).unwrap_or_default(),
            evidence: Default::default(),
            search: format!("Windows {}", r.group),
            verdict_by: r.verdict_by.clone(),
        });
    }
    let rule = engine.rule(&r.rule_id)?;
    let mut r = r;
    // Problems saved before a rule gained a cheap, offline check get it now.
    if rule.enrich.as_deref() == Some("bugcheck") && !r.evidence.contains_key("StopName") {
        crate::trust::enrich("bugcheck", &mut r.evidence);
    }
    // A remembered verdict (yours or the AI's) wins; then checks run on
    // the evidence may raise or lower the rule's defaults.
    let harmful = match r.verdict.as_str() {
        "yes" => Harm::Yes,
        "no" => Harm::No,
        "maybe" => Harm::Maybe,
        _ => match r.evidence.get("_harmful").map(String::as_str) {
        Some("yes") => Harm::Yes,
        Some("no") => Harm::No,
        Some("maybe") => Harm::Maybe,
        _ => rule.harmful,
        },
    };
    let severity = r
        .evidence
        .get("_severity")
        .and_then(|s| serde_json::from_str::<Level>(&format!("\"{s}\"")).ok())
        .unwrap_or(rule.severity);
    Some(Finding {
        id: r.id,
        rule_id: r.rule_id.clone(),
        group: r.group.clone(),
        title: rule.title.clone(),
        category: rule.category.clone(),
        severity,
        harmful,
        explanation: render(&rule.explain, &r.evidence, r.count),
        advice: rule.advice.clone(),
        message: r.evidence.get("_message").cloned().unwrap_or_default(),
        first_ts: r.first_ts,
        last_ts: r.last_ts,
        count: r.count,
        status: r.status,
        fixes: rule
            .fixes
            .iter()
            .enumerate()
            .map(|(index, f)| FixOption {
                index,
                label: f.label.clone(),
                action: f.action.clone(),
                risk: f.risk,
                needs_admin: actions::needs_admin(&f.action),
                undoable: actions::undoable(&f.action),
            })
            .collect(),
        attempts: store.attempts(r.id).unwrap_or_default(),
        search: if rule.search.is_empty() {
            format!("Windows {} event {}", r.evidence.get("provider").map(String::as_str).unwrap_or(""), r.evidence.get("event_id").map(String::as_str).unwrap_or(""))
        } else {
            render(&rule.search, &r.evidence, r.count).replace(" ?", "")
        },
        evidence: r.evidence.into_iter().filter(|(k, _)| !k.starts_with('_')).collect(),
        verdict_by: r.verdict_by,
    })
}

/// Remembers a harm verdict. The person can say anything; the AI may not
/// clear a security problem the rule rates as harmful (a wrong "it's fine"
/// about real malware would be worse than a false alarm).
fn set_verdict(engine: &Engine, store: &Store, finding: i64, harmful: &str, by: &str) -> Response {
    if !matches!(harmful, "" | "no" | "maybe" | "yes") || !matches!(by, "you" | "ai") {
        return Response::Error("bad verdict".into());
    }
    let row = match store.finding(finding) {
        Ok(Some(r)) => r,
        Ok(None) => return Response::Error("No such problem.".into()),
        Err(e) => return Response::Error(format!("store error: {e}")),
    };
    if by == "ai" {
        // The person's own verdict is never overwritten by the AI.
        if row.verdict_by == "you" {
            return Response::Done("Kept your own verdict.".into());
        }
        let protected = engine
            .rule(&row.rule_id)
            .is_some_and(|r| r.category == "security" && r.harmful == Harm::Yes);
        if protected && harmful != "yes" {
            return Response::Done("Security threats are only cleared by you.".into());
        }
    }
    match store.set_verdict(finding, harmful, by) {
        Ok(()) => Response::Done(match (harmful, by) {
            ("", _) => "Forgot the verdict.".into(),
            ("no", "you") => "Marked as safe. Syscura will not warn about this again.".into(),
            _ => "Saved.".into(),
        }),
        Err(e) => Response::Error(format!("store error: {e}")),
    }
}

fn status(store: &Store, shared: &Shared) -> syscura_store::Result<StatusInfo> {
    let (working_set_bytes, private_bytes) = meminfo::own_memory();
    Ok(StatusInfo {
        version: env!("CARGO_PKG_VERSION").into(),
        pid: std::process::id(),
        service: !shared.console,
        defender: shared.watch.defender(),
        uptime_secs: shared.started.elapsed().as_secs(),
        working_set_bytes,
        private_bytes,
        sensors: shared.sensors.clone(),
        events_total: store.count_events()?,
        last_24h: store.level_counts_since(now_ms() - 24 * 60 * 60 * 1000)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_reading_is_open_to_every_program() {
        let reads = [
            Request::Status,
            Request::Events { limit: 1, max_level: None },
            Request::Hardware { refresh: false },
            Request::Findings { include_closed: false },
            Request::Actions,
            Request::Processes,
            Request::RecheckFiles,
            Request::Security { refresh: true },
            Request::Options,
            Request::DiskHistory,
            Request::Summary { days: 7 },
        ];
        for r in &reads {
            assert!(!changes_something(r), "{} should be readable by anyone", request_name(r));
        }
        let changes = [
            Request::Fix { finding: 1, fix: 0 },
            Request::Undo { attempt: 1 },
            Request::Ignore { finding: 1, ignore: true },
            Request::SetVerdict { finding: 1, harmful: "no".into(), by: "you".into() },
            Request::ApplyAction {
                finding: None,
                title: String::new(),
                action: "service.disable".into(),
                params: Default::default(),
                label: String::new(),
                automatic: false,
            },
            Request::SetOption { key: "ransomware_canary".into(), value: "off".into() },
            Request::Shutdown,
        ];
        for r in &changes {
            assert!(changes_something(r), "{} must need Syscura itself or an admin", request_name(r));
        }
    }
}
