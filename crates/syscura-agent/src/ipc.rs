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
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE,
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
    let first = security.create_instance(true)?;
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
            let shutdown = match handle_client(&pipe, store.as_ref(), engine.as_ref(), &shared) {
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
fn handle_client(pipe: &File, store: Option<&Store>, engine: Option<&Engine>, shared: &Shared) -> std::io::Result<bool> {
    let mut line = String::new();
    BufReader::new(pipe.take(MAX_REQUEST)).read_line(&mut line)?;
    let mut shutdown = false;
    let response = match serde_json::from_str::<Request>(line.trim()) {
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
    let rule = engine.rule(&r.rule_id)?;
    // Checks run on the evidence may raise or lower the rule's defaults.
    let harmful = match r.evidence.get("_harmful").map(String::as_str) {
        Some("yes") => Harm::Yes,
        Some("no") => Harm::No,
        Some("maybe") => Harm::Maybe,
        _ => rule.harmful,
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
        evidence: r.evidence.into_iter().filter(|(k, _)| !k.starts_with('_')).collect(),
    })
}

fn status(store: &Store, shared: &Shared) -> syscura_store::Result<StatusInfo> {
    let (working_set_bytes, private_bytes) = meminfo::own_memory();
    Ok(StatusInfo {
        version: env!("CARGO_PKG_VERSION").into(),
        pid: std::process::id(),
        uptime_secs: shared.started.elapsed().as_secs(),
        working_set_bytes,
        private_bytes,
        sensors: shared.sensors.clone(),
        events_total: store.count_events()?,
        last_24h: store.level_counts_since(now_ms() - 24 * 60 * 60 * 1000)?,
    })
}
