//! The only things Syscura can change on the system.
//!
//! Every action is a fixed, reviewed operation. Rules, the user and the AI
//! can pick an action and fill in its parameters, but they can never run an
//! arbitrary command:
//! - programs are started by full System32 path, with fixed arguments;
//! - parameter values are validated before use;
//! - nothing is deleted: folders are renamed, settings are remembered, and
//!   every change that can be undone returns an undo token.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use syscura_core::findings::{ActionInfo, Risk};
use windows::Win32::System::Services::{
    ChangeServiceConfigW, CloseServiceHandle, ENUM_SERVICE_TYPE, GetServiceKeyNameW, OpenSCManagerW,
    SC_HANDLE, SC_MANAGER_CONNECT, SERVICE_ERROR, SERVICE_NO_CHANGE, SERVICE_START_TYPE,
};
use windows::core::{HSTRING, PCWSTR, PWSTR};
use windows_service::service::{ServiceAccess, ServiceStartType, ServiceState};
use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub struct Outcome {
    pub message: String,
    /// JSON undo token, when the change can be reversed.
    pub undo: Option<String>,
    pub effect: Effect,
}

/// What the action actually achieved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    /// It changed or repaired something.
    Fixed,
    /// It ran fine but found nothing wrong, so this was not the cause.
    NothingFound,
    /// It found damage it could not repair.
    NotRepaired,
}

impl Effect {
    pub fn as_str(self) -> &'static str {
        match self {
            Effect::Fixed => "fixed",
            Effect::NothingFound => "nothing_found",
            Effect::NotRepaired => "not_repaired",
        }
    }
}

/// Reads what a Windows repair tool reported, in plain words.
pub fn judge(action: &str, output: &str) -> (Effect, Option<&'static str>) {
    let o = output.to_ascii_lowercase();
    match action {
        "sfc.scan" if o.contains("did not find any integrity violations") => {
            (Effect::NothingFound, Some("Windows system files are healthy: SFC found nothing to repair, so they are not the cause."))
        }
        "sfc.scan" if o.contains("successfully repaired") => (Effect::Fixed, Some("SFC found damaged system files and repaired them.")),
        "sfc.scan" if o.contains("unable to fix") || o.contains("could not perform") => (
            Effect::NotRepaired,
            Some("SFC found damaged files it could not repair. Next: run 'Repair the Windows image (DISM)', then SFC again."),
        ),
        "dism.restore_health" if o.contains("no component store corruption detected") => {
            (Effect::NothingFound, Some("The Windows image is healthy: DISM found nothing to repair."))
        }
        "dism.restore_health" if o.contains("restore operation completed successfully") => {
            (Effect::Fixed, Some("DISM finished: the Windows image is repaired and healthy."))
        }
        "chkdsk.scan" if o.contains("found no problems") => {
            (Effect::NothingFound, Some("Drive C: has no file system errors (checked read-only)."))
        }
        "chkdsk.scan" if o.contains("found problems") || o.contains("errors found") => (
            Effect::NotRepaired,
            Some("chkdsk found file system errors on C:. Back up your files, then let Windows repair the drive at the next restart."),
        ),
        _ => (Effect::Fixed, None),
    }
}

/// Undo information, stored with the fix attempt.
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Undo {
    ServiceStartType { service: String, start_type: u32 },
    RenamedFolder { original: PathBuf, backup: PathBuf },
    MovedFiles { from: PathBuf, to: PathBuf, service: String },
}

/// (id, label, description, risk, params, undoable)
type Entry = (&'static str, &'static str, &'static str, Risk, &'static [&'static str], bool);

/// The fixed list. Risk is decided here, by Syscura, never by the caller.
const CATALOG: &[Entry] = &[
    ("service.ensure_running", "Start a stopped service", "Starts a Windows service if it is stopped. Leaves disabled services alone. Param service = service name or display name.", Risk::Safe, &["service"], false),
    ("service.restart", "Restart a service", "Stops and starts a running Windows service. Param service.", Risk::Caution, &["service"], false),
    ("service.disable", "Disable a service", "Sets a non-Windows service to Disabled and stops it (undo restores its start type). For unwanted or suspicious third-party services. Param service.", Risk::Risky, &["service"], true),
    ("dns.flush", "Clear the DNS cache", "Clears Windows' cache of website addresses (ipconfig /flushdns).", Risk::Safe, &[], false),
    ("net.renew", "Renew the network address", "Releases and renews the IP address from the router (ipconfig /release then /renew). The connection drops for a few seconds.", Risk::Caution, &[], false),
    ("winsock.reset", "Reset Winsock", "Resets the Windows network socket catalog (netsh winsock reset). Needs a restart.", Risk::Caution, &[], false),
    ("net.ip_reset", "Reset TCP/IP settings", "Resets the TCP/IP stack to defaults (netsh int ip reset). Custom static IP settings are lost. Needs a restart.", Risk::Risky, &[], false),
    ("time.resync", "Sync the clock", "Makes sure the time service runs and syncs the clock now.", Risk::Safe, &[], false),
    ("devices.rescan", "Rescan for hardware", "Asks Windows to detect hardware again, like Device Manager's 'Scan for hardware changes' (pnputil /scan-devices).", Risk::Safe, &[], false),
    ("print.clear_queue", "Clear stuck print jobs", "Stops the print spooler, moves stuck jobs to Syscura's quarantine (not deleted, can be undone) and starts the spooler again.", Risk::Caution, &[], true),
    ("defender.quick_scan", "Defender quick scan", "Runs a Microsoft Defender quick scan.", Risk::Safe, &[], false),
    ("defender.full_scan", "Defender full scan", "Runs a Microsoft Defender full scan (can take an hour or more).", Risk::Caution, &[], false),
    ("defender.update", "Update Defender", "Updates Microsoft Defender's virus definitions.", Risk::Safe, &[], false),
    ("defender.enable_realtime", "Turn real-time protection on", "Turns Microsoft Defender real-time protection back on.", Risk::Caution, &[], false),
    ("sfc.scan", "Repair system files (SFC)", "Runs System File Checker (sfc /scannow) to find and repair damaged Windows files. Takes 10-30 minutes.", Risk::Caution, &[], false),
    ("dism.restore_health", "Repair the Windows image (DISM)", "Runs DISM /Online /Cleanup-Image /RestoreHealth to repair Windows' component store. Takes 10-30 minutes.", Risk::Caution, &[], false),
    ("chkdsk.scan", "Check drive C: (read-only)", "Runs chkdsk on C: in read-only mode: reports file system problems, changes nothing.", Risk::Safe, &[], false),
    ("wu.reset_cache", "Reset Windows Update's cache", "Renames Windows Update's download cache so it is rebuilt (undo renames it back). Fixes many update failures.", Risk::Caution, &[], true),
    ("restore_point", "Create a restore point", "Creates a System Restore point.", Risk::Safe, &[], false),
];

pub fn catalog() -> Vec<ActionInfo> {
    CATALOG
        .iter()
        .map(|(id, label, description, risk, params, undoable)| ActionInfo {
            id: id.to_string(),
            label: label.to_string(),
            description: description.to_string(),
            risk: *risk,
            params: params.iter().map(|p| p.to_string()).collect(),
            undoable: *undoable,
            needs_admin: needs_admin(id),
        })
        .collect()
}

pub fn risk(action: &str) -> Option<Risk> {
    CATALOG.iter().find(|e| e.0 == action).map(|e| e.3)
}

/// Checks an action request from outside (UI or AI) before it is queued.
pub fn validate(action: &str, params: &BTreeMap<String, String>) -> Result<(), String> {
    let entry = CATALOG.iter().find(|e| e.0 == action).ok_or_else(|| format!("\"{action}\" is not one of Syscura's actions."))?;
    if let Some(k) = params.keys().find(|k| !entry.4.contains(&k.as_str())) {
        return Err(format!("\"{action}\" does not take a \"{k}\" setting."));
    }
    if let Some(s) = params.get("service") {
        valid_service_name(s)?;
    } else if entry.4.contains(&"service") {
        return Err("A service name is needed.".into());
    }
    Ok(())
}

/// Actions that need administrator rights (the service has them).
pub fn needs_admin(action: &str) -> bool {
    !matches!(action, "dns.flush")
}

pub fn undoable(action: &str) -> bool {
    CATALOG.iter().any(|e| e.0 == action && e.5)
}

/// Runs one action. `p` holds the rule's parameters, already filled in.
pub fn run(action: &str, p: &BTreeMap<String, String>) -> Result<Outcome, String> {
    let service = || p.get("service").map(String::as_str).ok_or_else(|| "missing service name".to_string()).and_then(valid_service_name);
    match action {
        "service.ensure_running" => ensure_running(service()?).map(done),
        "service.disable" => disable_service(service()?),
        "service.restart" => {
            let key = service_key(service()?)?;
            stop_and_wait(&key)?;
            ensure_running(&key).map(|m| done(format!("Restarted. {m}")))
        }
        "net.renew" => {
            tool(&system32("ipconfig.exe"), &["/release"], 120)?;
            tool(&system32("ipconfig.exe"), &["/renew"], 120).map(done)
        }
        "net.ip_reset" => tool(&system32("netsh.exe"), &["int", "ip", "reset"], 120)
            .map(|m| done(format!("{m} A restart is needed to finish."))),
        "devices.rescan" => tool(&system32("pnputil.exe"), &["/scan-devices"], 300).map(done),
        "print.clear_queue" => clear_print_queue(),
        "dns.flush" => tool(&system32("ipconfig.exe"), &["/flushdns"], 60).map(done),
        "time.resync" => {
            ensure_running("w32time")?;
            tool(&system32("w32tm.exe"), &["/resync"], 60).map(done)
        }
        "defender.quick_scan" => tool(&defender()?, &["-Scan", "-ScanType", "1"], 3600).map(done),
        "defender.full_scan" => tool(&defender()?, &["-Scan", "-ScanType", "2"], 6 * 3600).map(done),
        "defender.update" => tool(&defender()?, &["-SignatureUpdate"], 900).map(done),
        "defender.enable_realtime" => powershell("Set-MpPreference -DisableRealtimeMonitoring $false").map(done),
        "sfc.scan" => judged(action, tool_full(&system32("sfc.exe"), &["/scannow"], 3 * 3600)),
        "dism.restore_health" => {
            judged(action, tool_full(&system32("Dism.exe"), &["/Online", "/Cleanup-Image", "/RestoreHealth"], 3 * 3600))
        }
        // Without /f or /scan, chkdsk only reads and reports.
        "chkdsk.scan" => judged(action, tool_full(&system32("chkdsk.exe"), &["C:"], 3 * 3600)),
        "winsock.reset" => tool(&system32("netsh.exe"), &["winsock", "reset"], 120)
            .map(|m| done(format!("{m} A restart is needed to finish."))),
        "wu.reset_cache" => reset_update_cache(),
        "restore_point" => powershell(
            "Checkpoint-Computer -Description 'Syscura: before a fix' -RestorePointType MODIFY_SETTINGS",
        )
        .map(done),
        other => Err(format!("unknown action \"{other}\"")),
    }
}

fn done(message: String) -> Outcome {
    Outcome { message, undo: None, effect: Effect::Fixed }
}

/// Reverses a change recorded by `run`.
pub fn undo(token: &str) -> Result<String, String> {
    match serde_json::from_str::<Undo>(token).map_err(|e| format!("bad undo record: {e}"))? {
        Undo::ServiceStartType { service, start_type } => {
            set_start_type(&service, start_type)?;
            Ok(format!("Restored the start type of \"{service}\"."))
        }
        Undo::MovedFiles { from, to, service } => {
            with_services_stopped(&[service.as_str()], || {
                for entry in std::fs::read_dir(&to).map_err(|e| e.to_string())?.flatten() {
                    let dest = from.join(entry.file_name());
                    std::fs::rename(entry.path(), &dest).map_err(|e| format!("cannot move back {}: {e}", dest.display()))?;
                }
                Ok(())
            })?;
            Ok(format!("Moved the print jobs back to {}.", from.display()))
        }
        Undo::RenamedFolder { original, backup } => {
            if !backup.exists() {
                return Err(format!("{} is gone; nothing to restore.", backup.display()));
            }
            with_services_stopped(&["wuauserv", "bits"], || {
                if original.exists() {
                    // Keep the newer folder too, under another name.
                    let newer = original.with_extension(format!("syscura-newer-{}", syscura_core::now_ms()));
                    std::fs::rename(&original, &newer).map_err(|e| format!("cannot move the newer folder aside: {e}"))?;
                }
                std::fs::rename(&backup, &original).map_err(|e| format!("cannot restore {}: {e}", original.display()))
            })?;
            Ok(format!("Restored {}.", original.display()))
        }
    }
}

/// Checks used to verify a fix. Returns Ok(true) when the check passes.
pub fn probe(name: &str, p: &BTreeMap<String, String>) -> Result<bool, String> {
    match name {
        "service_running" => {
            let svc = valid_service_name(p.get("service").map(String::as_str).ok_or("missing service")?)?;
            let key = service_key(svc)?;
            let deadline = Instant::now() + Duration::from_secs(15);
            loop {
                if service_state(&key)? == ServiceState::Running {
                    return Ok(true);
                }
                if Instant::now() > deadline {
                    return Ok(false);
                }
                std::thread::sleep(Duration::from_millis(500));
            }
        }
        other => Err(format!("unknown check \"{other}\"")),
    }
}

// ------------------------------------------------------------- services

/// Event data is untrusted: allow only plausible service names.
fn valid_service_name(s: &str) -> Result<&str, String> {
    let ok = !s.is_empty()
        && s.len() <= 256
        && !s.chars().any(|c| c.is_control() || matches!(c, '\\' | '/' | '"' | '%'));
    if ok { Ok(s) } else { Err("not a valid service name".into()) }
}

/// Accepts a service key ("Spooler") or display name ("Print Spooler").
fn service_key(name: &str) -> Result<String, String> {
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT).map_err(err)?;
    if manager.open_service(name, ServiceAccess::QUERY_STATUS).is_ok() {
        return Ok(name.to_string());
    }
    unsafe {
        let scm = OpenSCManagerW(PCWSTR::null(), PCWSTR::null(), SC_MANAGER_CONNECT).map_err(err)?;
        let mut buf = [0u16; 257];
        let mut len = buf.len() as u32;
        let r = GetServiceKeyNameW(scm, &HSTRING::from(name), Some(PWSTR(buf.as_mut_ptr())), &mut len);
        let _ = CloseServiceHandle(scm);
        r.map_err(|_| format!("no service called \"{name}\""))?;
        Ok(String::from_utf16_lossy(&buf[..len as usize]))
    }
}

fn service_state(key: &str) -> Result<ServiceState, String> {
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT).map_err(err)?;
    let svc = manager.open_service(key, ServiceAccess::QUERY_STATUS).map_err(err)?;
    Ok(svc.query_status().map_err(err)?.current_state)
}

fn ensure_running(name: &str) -> Result<String, String> {
    let key = service_key(name)?;
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT).map_err(err)?;
    let svc = manager
        .open_service(&key, ServiceAccess::QUERY_STATUS | ServiceAccess::QUERY_CONFIG | ServiceAccess::START)
        .map_err(|e| format!("cannot open service \"{key}\": {}", err(e)))?;
    if svc.query_status().map_err(err)?.current_state == ServiceState::Running {
        return Ok(format!("\"{name}\" is running."));
    }
    // A disabled service was disabled on purpose; leave that decision alone.
    if svc.query_config().map_err(err)?.start_type == ServiceStartType::Disabled {
        return Err(format!("\"{name}\" is disabled, so Syscura did not start it."));
    }
    svc.start(&[] as &[&OsStr]).map_err(|e| format!("could not start \"{name}\": {}", err(e)))?;
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if svc.query_status().map_err(err)?.current_state == ServiceState::Running {
            return Ok(format!("Started \"{name}\"."));
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    Err(format!("\"{name}\" did not reach the running state within 30 seconds."))
}

fn disable_service(name: &str) -> Result<Outcome, String> {
    let key = service_key(name)?;
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT).map_err(err)?;
    let svc = manager
        .open_service(&key, ServiceAccess::QUERY_CONFIG | ServiceAccess::QUERY_STATUS | ServiceAccess::STOP)
        .map_err(|e| format!("cannot open service \"{key}\": {}", err(e)))?;
    let config = svc.query_config().map_err(err)?;
    // Never touch Windows' own components.
    let windir = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into()).to_ascii_lowercase();
    let image = config.executable_path.to_string_lossy().to_ascii_lowercase();
    if image.contains(&windir) || image.contains(r"\systemroot\") || image.starts_with("system32") {
        return Err(format!("\"{name}\" is part of Windows; Syscura will not disable it."));
    }
    let previous = start_type_raw(config.start_type);
    set_start_type(&key, 4)?;
    if svc.query_status().map(|s| s.current_state != ServiceState::Stopped).unwrap_or(false) {
        let _ = svc.stop();
    }
    let undo = serde_json::to_string(&Undo::ServiceStartType { service: key.clone(), start_type: previous }).ok();
    Ok(Outcome { message: format!("Disabled and stopped \"{name}\". Undo restores its previous start type."), undo, effect: Effect::Fixed })
}

fn start_type_raw(t: ServiceStartType) -> u32 {
    match t {
        ServiceStartType::BootStart => 0,
        ServiceStartType::SystemStart => 1,
        ServiceStartType::AutoStart => 2,
        ServiceStartType::OnDemand => 3,
        ServiceStartType::Disabled => 4,
    }
}

fn set_start_type(key: &str, start_type: u32) -> Result<(), String> {
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT).map_err(err)?;
    let svc = manager.open_service(key, ServiceAccess::CHANGE_CONFIG).map_err(|e| format!("cannot change \"{key}\": {}", err(e)))?;
    unsafe {
        ChangeServiceConfigW(
            SC_HANDLE(svc.raw_handle() as _),
            ENUM_SERVICE_TYPE(SERVICE_NO_CHANGE),
            SERVICE_START_TYPE(start_type),
            SERVICE_ERROR(SERVICE_NO_CHANGE),
            PCWSTR::null(),
            PCWSTR::null(),
            None,
            PCWSTR::null(),
            PCWSTR::null(),
            PCWSTR::null(),
            PCWSTR::null(),
        )
    }
    .map_err(|e| format!("could not change the start type: {e}"))
}

fn stop_and_wait(key: &str) -> Result<(), String> {
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT).map_err(err)?;
    let svc = manager.open_service(key, ServiceAccess::QUERY_STATUS | ServiceAccess::STOP).map_err(err)?;
    if svc.query_status().map_err(err)?.current_state == ServiceState::Stopped {
        return Ok(());
    }
    let _ = svc.stop();
    let deadline = Instant::now() + Duration::from_secs(60);
    while Instant::now() < deadline {
        if svc.query_status().map_err(err)?.current_state == ServiceState::Stopped {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    Err(format!("\"{key}\" did not stop"))
}

/// Stops the services, runs `f`, and starts them again whatever happened.
fn with_services_stopped<T>(keys: &[&str], f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    for k in keys {
        stop_and_wait(k)?;
    }
    let result = f();
    for k in keys.iter().rev() {
        let _ = ensure_running(k);
    }
    result
}

// ------------------------------------------------------------- Windows Update

/// Renames (never deletes) Windows Update's download cache so Windows
/// builds a fresh one. Undo renames it back.
fn reset_update_cache() -> Result<Outcome, String> {
    let windir = PathBuf::from(std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into()));
    let original = windir.join("SoftwareDistribution");
    let backup = windir.join(format!("SoftwareDistribution.syscura-{}", syscura_core::now_ms()));
    with_services_stopped(&["wuauserv", "bits"], || {
        std::fs::rename(&original, &backup).map_err(|e| format!("cannot rename {}: {e}", original.display()))
    })?;
    let undo = serde_json::to_string(&Undo::RenamedFolder { original: original.clone(), backup: backup.clone() }).ok();
    Ok(Outcome {
        message: format!("Moved the old cache to {}. Windows Update will rebuild it.", backup.display()),
        undo,
        effect: Effect::Fixed,
    })
}

// ------------------------------------------------------------- printing

/// Moves (never deletes) stuck print jobs out of the spool folder.
fn clear_print_queue() -> Result<Outcome, String> {
    let windir = PathBuf::from(std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into()));
    let spool = windir.join(r"System32\spool\PRINTERS");
    let base = std::env::var("ProgramData").unwrap_or_else(|_| r"C:\ProgramData".into());
    let dest = PathBuf::from(base).join("Syscura").join("quarantine").join(format!("print-jobs-{}", syscura_core::now_ms()));
    let mut moved = 0;
    with_services_stopped(&["spooler"], || {
        std::fs::create_dir_all(&dest).map_err(|e| format!("cannot create {}: {e}", dest.display()))?;
        for entry in std::fs::read_dir(&spool).map_err(|e| format!("cannot read {}: {e}", spool.display()))?.flatten() {
            if entry.path().is_file() {
                std::fs::rename(entry.path(), dest.join(entry.file_name())).map_err(|e| e.to_string())?;
                moved += 1;
            }
        }
        Ok(())
    })?;
    let undo = serde_json::to_string(&Undo::MovedFiles { from: spool, to: dest.clone(), service: "spooler".into() }).ok();
    Ok(Outcome { message: format!("Moved {moved} stuck print job file(s) to {}.", dest.display()), undo, effect: Effect::Fixed })
}

// ------------------------------------------------------------- tools

fn system32(exe: &str) -> PathBuf {
    let windir = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    PathBuf::from(windir).join("System32").join(exe)
}

fn defender() -> Result<PathBuf, String> {
    let pf = std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".into());
    let p = PathBuf::from(pf).join("Windows Defender").join("MpCmdRun.exe");
    if p.exists() { Ok(p) } else { Err("Microsoft Defender is not installed.".into()) }
}

fn powershell(script: &'static str) -> Result<String, String> {
    let ps = system32(r"WindowsPowerShell\v1.0\powershell.exe");
    tool(&ps, &["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script], 600)
}

/// Turns a repair tool's full output into an honest outcome.
fn judged(action: &str, out: Result<String, String>) -> Result<Outcome, String> {
    let full = match out {
        Ok(text) => text,
        // sfc/chkdsk use non-zero exit codes for "found problems" too.
        Err(e) if e.contains("integrity violations") || e.contains("found problems") || e.contains("unable to fix") => e,
        Err(e) => return Err(e),
    };
    let (effect, plain) = judge(action, &full);
    let summary = last_lines(&full, 2);
    let message = match plain {
        Some(p) => format!("{p} ({summary})"),
        None => summary,
    };
    Ok(Outcome { message, undo: None, effect })
}

/// Like `tool`, but returns the complete output (for tools whose result
/// has to be read).
fn tool_full(exe: &PathBuf, args: &[&str], timeout_secs: u64) -> Result<String, String> {
    run_tool(exe, args, timeout_secs, true)
}

/// Runs a Windows tool hidden, with a time limit. Returns its last lines.
fn tool(exe: &PathBuf, args: &[&str], timeout_secs: u64) -> Result<String, String> {
    run_tool(exe, args, timeout_secs, false)
}

fn run_tool(exe: &PathBuf, args: &[&str], timeout_secs: u64, full: bool) -> Result<String, String> {
    let name = exe.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut child = Command::new(exe)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| format!("cannot start {name}: {e}"))?;
    let out_pipe = child.stdout.take();
    let err_pipe = child.stderr.take();
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut o) = out_pipe {
            let _ = std::io::Read::read_to_end(&mut o, &mut buf);
        }
        if let Some(mut e) = err_pipe {
            let _ = std::io::Read::read_to_end(&mut e, &mut buf);
        }
        buf
    });
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    let status = loop {
        if let Some(s) = child.try_wait().map_err(err)? {
            break s;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            return Err(format!("{name} did not finish in time and was stopped."));
        }
        std::thread::sleep(Duration::from_millis(250));
    };
    let text = decode(&reader.join().unwrap_or_default());
    let summary = if full { text } else { last_lines(&text, 3) };
    if status.success() {
        Ok(if summary.trim().is_empty() { format!("{name} finished.") } else { summary })
    } else {
        Err(format!("{name} failed (exit code {}). {}", status.code().unwrap_or(-1), if full { summary } else { summary.clone() }))
    }
}

/// Some tools (sfc) write UTF-16; most write the console code page.
fn decode(bytes: &[u8]) -> String {
    let zeros = bytes.iter().skip(1).step_by(2).filter(|&&b| b == 0).count();
    if bytes.len() > 4 && zeros * 2 > bytes.len() / 2 {
        let wide: Vec<u16> = bytes.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
        String::from_utf16_lossy(&wide)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

/// The last meaningful lines of a tool's output. Progress updates
/// ("Verification 45% complete.", "[==   20.0%   ]") are dropped; tools
/// redraw them with carriage returns, so those count as line breaks too.
fn last_lines(s: &str, n: usize) -> String {
    let lines: Vec<&str> = s
        .split(['\r', '\n'])
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.contains("% complete") && !l.ends_with('%') && !l.starts_with('['))
        .collect();
    let mut keep: Vec<&str> = lines[lines.len().saturating_sub(n)..].to_vec();
    keep.dedup();
    keep.join(" ")
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_names_are_checked() {
        assert!(valid_service_name("Spooler").is_ok());
        assert!(valid_service_name("Print Spooler").is_ok());
        assert!(valid_service_name("").is_err());
        assert!(valid_service_name("evil\\..\\x").is_err());
        assert!(valid_service_name("a\"b").is_err());
        assert!(valid_service_name(&"x".repeat(300)).is_err());
    }

    #[test]
    fn catalog_is_complete_and_validation_is_strict() {
        // Every action a rule uses is in the catalog (checked statically:
        // running actions in tests would change the machine).
        let ids: Vec<String> = catalog().into_iter().map(|a| a.id).collect();
        for rule in syscura_rules::Engine::builtin().unwrap().rules() {
            for f in &rule.fixes {
                assert!(ids.contains(&f.action), "rule {} uses {} which is not in the catalog", rule.id, f.action);
            }
        }
        let p = |k: &str, v: &str| BTreeMap::from([(k.to_string(), v.to_string())]);
        assert!(validate("service.ensure_running", &p("service", "Spooler")).is_ok());
        assert!(validate("service.ensure_running", &BTreeMap::new()).is_err(), "service is required");
        assert!(validate("dns.flush", &p("command", "format c:")).is_err(), "unknown parameter");
        assert!(validate("cmd.run", &BTreeMap::new()).is_err(), "not in the catalog");
        assert!(validate("service.disable", &p("service", "a\\b")).is_err(), "bad service name");
        assert_eq!(risk("service.disable"), Some(Risk::Risky));
        assert_eq!(risk("nope"), None);
    }

    #[test]
    fn unknown_actions_are_refused() {
        assert!(run("cmd.run", &BTreeMap::new()).is_err());
        assert!(run("service.ensure_running", &BTreeMap::new()).is_err(), "missing parameter");
    }

    #[test]
    fn undo_tokens_round_trip() {
        let t = serde_json::to_string(&Undo::ServiceStartType { service: "x".into(), start_type: 3 }).unwrap();
        assert_eq!(t, r#"{"kind":"service_start_type","service":"x","start_type":3}"#);
        assert!(undo("not json").is_err());
    }

    #[test]
    fn output_decoding() {
        let wide: Vec<u8> = "ok\r\nVerification 100% complete.\r\nNo integrity violations.\r\n"
            .encode_utf16()
            .flat_map(|c| c.to_le_bytes())
            .collect();
        assert_eq!(last_lines(&decode(&wide), 2), "ok No integrity violations.");
        assert_eq!(last_lines(&decode(b"line1\nline2\n"), 1), "line2");
        let sfc = "Beginning verification phase of system scan.\r\nVerification 1% complete.\rVerification 99% complete.\r\n\r\nWindows Resource Protection did not find any integrity violations.\r\n";
        assert_eq!(last_lines(sfc, 2), "Beginning verification phase of system scan. Windows Resource Protection did not find any integrity violations.");
    }

    #[test]
    fn repair_results_are_read_honestly() {
        let (e, _) = judge("sfc.scan", "Windows Resource Protection did not find any integrity violations.");
        assert_eq!(e, Effect::NothingFound);
        let (e, _) = judge("sfc.scan", "Windows Resource Protection found corrupt files and successfully repaired them.");
        assert_eq!(e, Effect::Fixed);
        let (e, msg) = judge("sfc.scan", "Windows Resource Protection found corrupt files but was unable to fix some of them.");
        assert_eq!(e, Effect::NotRepaired);
        assert!(msg.unwrap().contains("DISM"));
        assert_eq!(judge("dism.restore_health", "No component store corruption detected.").0, Effect::NothingFound);
        assert_eq!(judge("chkdsk.scan", "Windows has scanned the file system and found no problems.").0, Effect::NothingFound);
        assert_eq!(judge("dns.flush", "Successfully flushed the DNS Resolver Cache.").0, Effect::Fixed);
    }
}
