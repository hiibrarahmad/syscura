//! Syscura's own checks, beyond Windows' event logs:
//! - a light **process watch**: every 2 seconds a process snapshot (the same
//!   cheap list Task Manager reads); only programs not seen before are
//!   checked, and each program file's signature is checked once and cached;
//! - a scan of everything that **starts with Windows** (Run keys, Startup
//!   folders, scheduled tasks), where most malware keeps itself running;
//! - **Defender's status** every half hour.
//!
//! Suspicious things become events on the "Syscura/Processes" and
//! "Syscura/Startup" channels, which the rules in kb/rules.toml turn into
//! problems like any Windows event.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use syscura_core::{DefenderInfo, Event, Level, ProcessInfo, now_ms};
use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use windows::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::core::{HSTRING, PWSTR};

use crate::reg::{self, HKEY_LOCAL_MACHINE, HKEY_USERS, KEY_WOW64_32KEY, VIEW64 as KEY_WOW64_64KEY};

use crate::agent::Msg;
use crate::trust::{self, Location, Signature};
use crate::log;

const POLL: Duration = Duration::from_secs(2);
const STARTUP_SCAN_EVERY: Duration = Duration::from_secs(6 * 60 * 60);
const DEFENDER_EVERY: Duration = Duration::from_secs(30 * 60);
/// Signatures of every listed program are only checked while someone
/// looks at the Processes page.
const VIEWER_WINDOW: Duration = Duration::from_secs(60);
const BATCH: usize = 40;

/// Names of Windows' own programs that malware likes to copy.
const SYSTEM_NAMES: &[&str] = &[
    "svchost.exe", "lsass.exe", "csrss.exe", "winlogon.exe", "services.exe", "smss.exe", "wininit.exe",
    "lsm.exe", "spoolsv.exe", "taskhostw.exe", "dllhost.exe", "conhost.exe", "explorer.exe", "rundll32.exe",
    "dwm.exe", "sihost.exe", "ctfmon.exe", "searchindexer.exe", "wuauclt.exe",
];
/// Office and PDF programs: documents should never start a shell.
const DOCUMENT_APPS: &[&str] = &[
    "winword.exe", "excel.exe", "powerpnt.exe", "outlook.exe", "msaccess.exe", "mspub.exe", "onenote.exe",
    "visio.exe", "acrord32.exe", "acrobat.exe", "foxitpdfreader.exe", "wordpad.exe",
];
const SHELLS: &[&str] = &[
    "powershell.exe", "pwsh.exe", "cmd.exe", "wscript.exe", "cscript.exe", "mshta.exe", "rundll32.exe",
    "regsvr32.exe", "certutil.exe", "bitsadmin.exe",
];
/// Script hosts whose command line deserves a look in startup items.
const SCRIPT_HOSTS: &[&str] = &["powershell.exe", "pwsh.exe", "cmd.exe", "wscript.exe", "cscript.exe", "mshta.exe"];
const SCRIPT_RED_FLAGS: &[&str] = &[
    "-enc", "-encodedcommand", "frombase64string", "downloadstring", "downloadfile", "invoke-webrequest", "iwr ",
    "invoke-expression", "iex ", "http://", "https://", "-w hidden", "-windowstyle hidden", "javascript:", "vbscript:",
];

#[derive(Clone)]
struct Proc {
    name: String,
    path: String,
    parent: u32,
    threads: u32,
    created_ms: i64,
    warning: String,
}

#[derive(Default)]
pub struct Watch {
    procs: Mutex<HashMap<u32, Proc>>,
    sigs: Mutex<HashMap<String, Signature>>,
    defender: Mutex<Option<DefenderInfo>>,
    /// CPU time per process at the previous listing, for CPU %.
    cpu_prev: Mutex<(Option<Instant>, HashMap<u32, u64>)>,
    last_viewed: Mutex<Option<Instant>>,
    /// Driver file hashes, so unchanged drivers are not read again.
    pub drivers: crate::checks::DriverCache,
    /// The latest security settings check.
    pub security: Mutex<Option<syscura_core::SecurityReport>>,
    security_busy: Mutex<bool>,
}

impl Watch {
    pub fn defender(&self) -> Option<DefenderInfo> {
        lock(&self.defender).clone()
    }

    /// The latest security settings check. A new check (when asked, or
    /// when there is none yet) runs on its own thread, so the pipe stays
    /// free; the answer says `refreshing` until it is done.
    pub fn security(self: &Arc<Self>, refresh: bool) -> syscura_core::SecurityReport {
        let current = lock(&self.security).clone();
        if !refresh && let Some(r) = current {
            return r;
        }
        let mut busy = lock(&self.security_busy);
        if !*busy {
            *busy = true;
            let me = self.clone();
            let spawned = std::thread::Builder::new().name("security".into()).stack_size(512 * 1024).spawn(move || {
                let r = crate::posture::report(me.defender().or_else(defender_status));
                *lock(&me.security) = Some(r);
                *lock(&me.security_busy) = false;
            });
            if spawned.is_err() {
                *busy = false;
            }
        }
        let mut r = current.unwrap_or_default();
        r.refreshing = true;
        r
    }

    /// Every running program with details. Memory and CPU are read now,
    /// only for this listing.
    pub fn processes(&self) -> Vec<ProcessInfo> {
        *lock(&self.last_viewed) = Some(Instant::now());
        let procs = lock(&self.procs).clone();
        let sigs = lock(&self.sigs).clone();
        let mut prev = lock(&self.cpu_prev);
        let elapsed_100ns = prev.0.map(|t| t.elapsed().as_nanos() as f64 / 100.0).unwrap_or(0.0);
        let cpus = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1) as f64;
        let mut now_cpu = HashMap::new();
        let mut out: Vec<ProcessInfo> = procs
            .iter()
            .map(|(&pid, p)| {
                let (memory_bytes, cpu) = usage(pid);
                let cpu_pct = match (cpu, prev.1.get(&pid)) {
                    (Some(c), Some(&before)) if elapsed_100ns > 0.0 && c >= before => {
                        ((c - before) as f64 / elapsed_100ns / cpus * 100.0).min(100.0) as f32
                    }
                    _ => 0.0,
                };
                if let Some(c) = cpu {
                    now_cpu.insert(pid, c);
                }
                let (signature, signer) = match sigs.get(&p.path.to_ascii_lowercase()) {
                    Some(Signature::Valid(s)) => ("valid", s.clone()),
                    Some(Signature::Missing) => ("unsigned", String::new()),
                    Some(Signature::Invalid(why)) => ("invalid", why.clone()),
                    Some(Signature::Unknown) => ("unknown", String::new()),
                    None => ("", String::new()),
                };
                ProcessInfo {
                    pid,
                    parent: p.parent,
                    parent_name: procs.get(&p.parent).map(|x| x.name.clone()).unwrap_or_default(),
                    name: p.name.clone(),
                    path: p.path.clone(),
                    started_ms: p.created_ms,
                    memory_bytes,
                    cpu_pct,
                    threads: p.threads,
                    signature: signature.into(),
                    signer,
                    location: location_name(&p.path).into(),
                    warning: p.warning.clone(),
                }
            })
            .collect();
        *prev = (Some(Instant::now()), now_cpu);
        out.sort_by_key(|p| std::cmp::Reverse(p.memory_bytes));
        out
    }
}

pub(crate) fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn location_name(path: &str) -> &'static str {
    if path.is_empty() {
        return "other";
    }
    match trust::classify_location(path) {
        Location::System => "system",
        Location::ProgramFiles => "program_files",
        Location::UserWritable => "user",
        Location::Other => "other",
    }
}

pub fn start(watch: Arc<Watch>, tx: Sender<Msg>) {
    let spawned = std::thread::Builder::new().name("watch".into()).stack_size(512 * 1024).spawn(move || run(&watch, &tx));
    if let Err(e) = spawned {
        log::error(&format!("cannot start the process watch: {e}"));
    }
}

fn run(watch: &Watch, tx: &Sender<Msg>) {
    let started = Instant::now();
    let mut last_startup_scan: Option<Instant> = None;
    let mut last_defender: Option<Instant> = None;
    // Processes waiting for their signature before they can be judged.
    let mut waiting: Vec<u32> = Vec::new();
    loop {
        let fresh = refresh(watch, tx);
        for pid in fresh {
            let p = lock(&watch.procs).get(&pid).cloned();
            if let Some(p) = p
                && !p.path.is_empty()
                && trust::classify_location(&p.path) == Location::UserWritable
                && is_program(&p.path)
            {
                waiting.push(pid);
            }
        }
        // Signatures: first those needed for a warning, then (only while
        // someone looks at the Processes page) the rest of the list.
        let mut need: Vec<String> = Vec::new();
        {
            let procs = lock(&watch.procs);
            let sigs = lock(&watch.sigs);
            for pid in &waiting {
                if let Some(p) = procs.get(pid) {
                    let key = p.path.to_ascii_lowercase();
                    if !sigs.contains_key(&key) && !need.iter().any(|n| n.eq_ignore_ascii_case(&p.path)) {
                        need.push(p.path.clone());
                    }
                }
            }
            let viewing = lock(&watch.last_viewed).is_some_and(|t| t.elapsed() < VIEWER_WINDOW);
            if viewing {
                for p in procs.values() {
                    if need.len() >= BATCH {
                        break;
                    }
                    if !p.path.is_empty() && !sigs.contains_key(&p.path.to_ascii_lowercase()) && !need.iter().any(|n| n.eq_ignore_ascii_case(&p.path)) {
                        need.push(p.path.clone());
                    }
                }
            }
        }
        need.truncate(BATCH);
        if !need.is_empty() {
            let checked = trust::signatures(&need);
            let mut sigs = lock(&watch.sigs);
            for (path, sig) in checked {
                sigs.insert(path.to_ascii_lowercase(), sig);
            }
        }
        waiting.retain(|pid| {
            let p = lock(&watch.procs).get(pid).cloned();
            let Some(p) = p else { return false };
            let sig = lock(&watch.sigs).get(&p.path.to_ascii_lowercase()).cloned();
            match sig {
                None => true, // still waiting
                Some(Signature::Valid(_)) | Some(Signature::Unknown) => false,
                Some(other) => {
                    let why = match &other {
                        Signature::Missing => "not signed".to_string(),
                        Signature::Invalid(w) => format!("signature NOT valid: {w}"),
                        _ => String::new(),
                    };
                    set_warning(watch, *pid, "Runs from a user folder without a valid signature");
                    emit(tx, "Syscura/Processes", 2, Level::Warning, &format!("{}|{}", p.path, p.created_ms), [
                        ("Name", p.name.clone()),
                        ("Path", p.path.clone()),
                        ("Pid", pid.to_string()),
                        ("Signature", why),
                    ]);
                    false
                }
            }
        });

        if started.elapsed() > Duration::from_secs(20) && last_defender.is_none_or(|t| t.elapsed() >= DEFENDER_EVERY) {
            *lock(&watch.defender) = defender_status();
            last_defender = Some(Instant::now());
        }
        if started.elapsed() > Duration::from_secs(60) && last_startup_scan.is_none_or(|t| t.elapsed() >= STARTUP_SCAN_EVERY) {
            startup_scan(tx);
            crate::checks::scan_all(tx, &watch.drivers);
            *lock(&watch.security) = Some(crate::posture::report(watch.defender()));
            last_startup_scan = Some(Instant::now());
        }
        std::thread::sleep(POLL);
    }
}

fn set_warning(watch: &Watch, pid: u32, text: &str) {
    if let Some(p) = lock(&watch.procs).get_mut(&pid) {
        p.warning = text.to_string();
    }
}

pub(crate) fn is_program(path: &str) -> bool {
    let p = path.to_ascii_lowercase();
    [".exe", ".scr", ".com", ".pif"].iter().any(|e| p.ends_with(e))
}

pub(crate) fn emit<const N: usize>(tx: &Sender<Msg>, channel: &str, event_id: u32, level: Level, key: &str, data: [(&str, String); N]) {
    // Stable record ids: the same finding is not stored twice.
    let record_id = key.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3));
    let _ = tx.send(Msg::Event(Event {
        ts: now_ms(),
        source: "syscura".into(),
        channel: channel.into(),
        provider: "Syscura".into(),
        event_id,
        level,
        record_id: record_id >> 1,
        data: data.into_iter().map(|(k, v)| (k.to_string(), v)).collect::<BTreeMap<_, _>>(),
    }));
}

// ------------------------------------------------------------ processes

/// Takes a snapshot, records new processes and checks them. Returns the
/// new process ids.
fn refresh(watch: &Watch, tx: &Sender<Msg>) -> Vec<u32> {
    let snap = snapshot();
    if snap.is_empty() {
        return Vec::new();
    }
    let alive: HashSet<u32> = snap.iter().map(|s| s.0).collect();
    let mut fresh = Vec::new();
    {
        let mut procs = lock(&watch.procs);
        procs.retain(|pid, _| alive.contains(pid));
        for &(pid, parent, ref name, threads) in &snap {
            if let Some(p) = procs.get_mut(&pid) {
                p.threads = threads;
                continue;
            }
            let (path, created_ms) = identity(pid);
            procs.insert(pid, Proc { name: name.clone(), path, parent, threads, created_ms, warning: String::new() });
            fresh.push(pid);
        }
    }
    let first_run = lock(&watch.cpu_prev).0.is_none() && fresh.len() == snap.len();
    let procs = lock(&watch.procs).clone();
    for pid in &fresh {
        let Some(p) = procs.get(pid) else { continue };
        let lname = p.name.to_ascii_lowercase();
        // 1. A Windows program name running from outside Windows' folders.
        if !p.path.is_empty() && SYSTEM_NAMES.contains(&lname.as_str()) && !in_windows_dir(&p.path) {
            set_warning(watch, *pid, "Pretends to be part of Windows");
            emit(tx, "Syscura/Processes", 1, Level::Critical, &format!("{}|{}", p.path, p.created_ms), [
                ("Name", p.name.clone()),
                ("Path", p.path.clone()),
                ("Pid", pid.to_string()),
            ]);
        }
        // 2. A document program starting a shell (not on the first
        // snapshot: parents of old processes may be long gone or reused).
        if !first_run
            && SHELLS.contains(&lname.as_str())
            && let Some(parent) = procs.get(&p.parent)
            && DOCUMENT_APPS.contains(&parent.name.to_ascii_lowercase().as_str())
            && parent.created_ms <= p.created_ms
        {
            set_warning(watch, *pid, &format!("Started by {}", parent.name));
            emit(tx, "Syscura/Processes", 3, Level::Error, &format!("{}|{}", pid, p.created_ms), [
                ("Name", p.name.clone()),
                ("Path", p.path.clone()),
                ("Pid", pid.to_string()),
                ("Parent", parent.name.clone()),
                ("ParentPath", parent.path.clone()),
            ]);
        }
    }
    fresh
}

fn in_windows_dir(path: &str) -> bool {
    let windir = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into()).to_ascii_lowercase();
    let p = path.to_ascii_lowercase();
    let parent = Path::new(&p).parent().map(|d| d.to_string_lossy().to_string()).unwrap_or_default();
    parent == windir
        || p.starts_with(&format!("{windir}\\system32\\"))
        || p.starts_with(&format!("{windir}\\syswow64\\"))
        || p.starts_with(&format!("{windir}\\winsxs\\"))
        || p.starts_with(&format!("{windir}\\systemapps\\"))
        || p.starts_with(&format!("{windir}\\immersivecontrolpanel\\"))
}

/// (pid, parent pid, exe name, threads) for every process.
fn snapshot() -> Vec<(u32, u32, String, u32)> {
    let mut out = Vec::new();
    unsafe {
        let Ok(h) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return out };
        let mut e = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        if Process32FirstW(h, &mut e).is_ok() {
            loop {
                let len = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
                out.push((e.th32ProcessID, e.th32ParentProcessID, String::from_utf16_lossy(&e.szExeFile[..len]), e.cntThreads));
                if Process32NextW(h, &mut e).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(h);
    }
    out
}

fn open(pid: u32) -> Option<HANDLE> {
    if pid == 0 || pid == 4 {
        return None; // System Idle Process and System
    }
    unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok() }
}

fn filetime_100ns(t: FILETIME) -> u64 {
    ((t.dwHighDateTime as u64) << 32) | t.dwLowDateTime as u64
}

/// Program path and start time (Unix ms).
fn identity(pid: u32) -> (String, i64) {
    let Some(h) = open(pid) else { return (String::new(), 0) };
    let mut path = String::new();
    let mut created_ms = 0;
    unsafe {
        let mut buf = vec![0u16; 1024];
        let mut len = buf.len() as u32;
        if QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok() {
            path = String::from_utf16_lossy(&buf[..len as usize]);
        }
        let (mut c, mut e, mut k, mut u) = Default::default();
        if GetProcessTimes(h, &mut c, &mut e, &mut k, &mut u).is_ok() {
            created_ms = (filetime_100ns(c) as i64 - 116_444_736_000_000_000) / 10_000;
        }
        let _ = CloseHandle(h);
    }
    (path, created_ms)
}

/// Working set bytes and total CPU time (100 ns units).
fn usage(pid: u32) -> (u64, Option<u64>) {
    let Some(h) = open(pid) else { return (0, None) };
    let mut mem = 0;
    let mut cpu = None;
    unsafe {
        let mut pmc = PROCESS_MEMORY_COUNTERS { cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32, ..Default::default() };
        if GetProcessMemoryInfo(h, &mut pmc, pmc.cb).is_ok() {
            mem = pmc.WorkingSetSize as u64;
        }
        let (mut c, mut e, mut k, mut u) = Default::default();
        if GetProcessTimes(h, &mut c, &mut e, &mut k, &mut u).is_ok() {
            cpu = Some(filetime_100ns(k) + filetime_100ns(u));
        }
        let _ = CloseHandle(h);
    }
    (mem, cpu)
}

// ------------------------------------------------------- startup items

struct StartupItem {
    name: String,
    /// "Run key (all users)", "Startup folder", "Scheduled task", ...
    place: String,
    command: String,
}

fn startup_scan(tx: &Sender<Msg>) {
    let items = startup_items();
    let mut programs: Vec<String> = Vec::new();
    let mut parsed = Vec::new();
    for it in items {
        let exe = trust::image_file(&expand(&it.command));
        let exe_name = Path::new(&exe).file_name().map(|n| n.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        let lower = it.command.to_ascii_lowercase();
        // A script host with tell-tale arguments: hidden, encoded or downloading.
        if SCRIPT_HOSTS.contains(&exe_name.as_str())
            && let Some(flag) = SCRIPT_RED_FLAGS.iter().find(|f| lower.contains(*f))
        {
            emit(tx, "Syscura/Startup", 11, Level::Error, &format!("script|{}|{}", it.place, it.command), [
                ("Name", it.name.clone()),
                ("Where", it.place.clone()),
                ("Command", it.command.chars().take(400).collect()),
                ("Flag", flag.trim().to_string()),
            ]);
        }
        if trust::classify_location(&exe) == Location::UserWritable && !programs.contains(&exe) {
            programs.push(exe.clone());
        }
        parsed.push((it, exe));
    }
    let mut sigs = HashMap::new();
    for chunk in programs.chunks(BATCH) {
        sigs.extend(trust::signatures(chunk));
    }
    let mut flagged = 0;
    for (it, exe) in parsed {
        let why = match sigs.get(&exe) {
            Some(Signature::Missing) => "not signed".to_string(),
            Some(Signature::Invalid(w)) => format!("signature NOT valid: {w}"),
            _ => continue,
        };
        flagged += 1;
        emit(tx, "Syscura/Startup", 10, Level::Warning, &format!("item|{}|{}", it.place, exe), [
            ("Name", it.name),
            ("Where", it.place),
            ("Path", exe),
            ("Signature", why),
        ]);
    }
    log::info(&format!("checked programs that start with Windows: {flagged} unsigned in user folders"));
}

pub(crate) fn expand(s: &str) -> String {
    use windows::Win32::System::Environment::ExpandEnvironmentStringsW;
    let src = HSTRING::from(s);
    let mut buf = vec![0u16; 2048];
    let n = unsafe { ExpandEnvironmentStringsW(&src, Some(&mut buf)) } as usize;
    if n == 0 || n > buf.len() {
        return s.to_string();
    }
    String::from_utf16_lossy(&buf[..n.saturating_sub(1)])
}

fn startup_items() -> Vec<StartupItem> {
    let mut out = Vec::new();
    let run = r"Software\Microsoft\Windows\CurrentVersion\Run";
    let run_once = r"Software\Microsoft\Windows\CurrentVersion\RunOnce";
    for (key, view, place) in [
        (run, KEY_WOW64_64KEY, "Run key (all users)"),
        (run, KEY_WOW64_32KEY, "Run key (all users, 32-bit)"),
        (run_once, KEY_WOW64_64KEY, "RunOnce key (all users)"),
    ] {
        for (name, cmd) in reg::values(HKEY_LOCAL_MACHINE, key, view) {
            out.push(StartupItem { name, place: place.into(), command: cmd });
        }
    }
    // Every signed-in user's own Run keys.
    for sid in reg::user_sids() {
        for (key, place) in [(run, "Run key (user)"), (run_once, "RunOnce key (user)")] {
            for (name, cmd) in reg::values(HKEY_USERS, &format!(r"{sid}\{key}"), KEY_WOW64_64KEY) {
                out.push(StartupItem { name, place: place.into(), command: cmd });
            }
        }
    }
    // Programs placed straight into a Startup folder (shortcuts are normal).
    let program_data = std::env::var("ProgramData").unwrap_or_else(|_| r"C:\ProgramData".into());
    let mut folders = vec![PathBuf::from(program_data).join(r"Microsoft\Windows\Start Menu\Programs\StartUp")];
    let users = std::env::var("SystemDrive").map(|d| PathBuf::from(format!("{d}\\Users"))).unwrap_or_else(|_| PathBuf::from(r"C:\Users"));
    if let Ok(dir) = std::fs::read_dir(users) {
        for u in dir.flatten() {
            folders.push(u.path().join(r"AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Startup"));
        }
    }
    for f in folders {
        let Ok(dir) = std::fs::read_dir(&f) else { continue };
        for e in dir.flatten() {
            let p = e.path();
            let ext = p.extension().map(|x| x.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
            if ["exe", "bat", "cmd", "vbs", "js", "ps1", "scr", "hta"].contains(&ext.as_str()) {
                out.push(StartupItem {
                    name: e.file_name().to_string_lossy().to_string(),
                    place: "Startup folder".into(),
                    command: format!("\"{}\"", p.display()),
                });
            }
        }
    }
    // Scheduled tasks: the XML files Task Scheduler keeps.
    let windir = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    let tasks = PathBuf::from(windir).join(r"System32\Tasks");
    let mut stack = vec![tasks.clone()];
    let mut seen = 0;
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            seen += 1;
            if seen > 3000 {
                break;
            }
            let Ok(bytes) = std::fs::read(&p) else { continue };
            let xml = decode_xml(&bytes);
            let name = p.strip_prefix(&tasks).map(|r| r.display().to_string()).unwrap_or_default();
            for (cmd, args) in task_commands(&xml) {
                let command = if cmd.starts_with('"') { format!("{cmd} {args}") } else { format!("\"{cmd}\" {args}") };
                out.push(StartupItem { name: name.clone(), place: "Scheduled task".into(), command: command.trim().to_string() });
            }
        }
    }
    out
}

fn decode_xml(bytes: &[u8]) -> String {
    if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE {
        let words: Vec<u16> = bytes[2..].as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
        String::from_utf16_lossy(&words)
    } else {
        String::from_utf8_lossy(bytes).trim_start_matches('\u{feff}').to_string()
    }
}

/// (Command, Arguments) of every Exec action in a task's XML.
fn task_commands(xml: &str) -> Vec<(String, String)> {
    let tag = |s: &str, name: &str| -> Option<String> {
        let open = format!("<{name}>");
        let start = s.find(&open)? + open.len();
        let end = s[start..].find(&format!("</{name}>"))? + start;
        Some(unescape(s[start..end].trim()))
    };
    xml.split("<Exec>")
        .skip(1)
        .filter_map(|part| {
            let part = part.split("</Exec>").next().unwrap_or(part);
            Some((tag(part, "Command")?, tag(part, "Arguments").unwrap_or_default()))
        })
        .collect()
}

fn unescape(s: &str) -> String {
    s.replace("&quot;", "\"").replace("&apos;", "'").replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&")
}

/// One pass of every check, printed: `syscura-agent selftest`. Changes
/// nothing; used to try Syscura's own checks on a machine (and in CI).
pub fn self_test() {
    let (tx, rx) = std::sync::mpsc::channel();
    let watch = Watch::default();
    let fresh = refresh(&watch, &tx);
    let procs = lock(&watch.procs).clone();
    let with_path = procs.values().filter(|p| !p.path.is_empty()).count();
    println!("processes: {} running, path known for {with_path}", procs.len());
    let user: Vec<String> = procs
        .values()
        .filter(|p| !p.path.is_empty() && trust::classify_location(&p.path) == Location::UserWritable && is_program(&p.path))
        .map(|p| p.path.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let t = Instant::now();
    let sigs = trust::signatures(&user.iter().take(BATCH).cloned().collect::<Vec<_>>());
    println!("programs in user folders: {} (signatures of {} checked in {} ms)", user.len(), sigs.len(), t.elapsed().as_millis());
    for (path, sig) in &sigs {
        if !matches!(sig, Signature::Valid(_)) {
            println!("  unsigned: {path} ({sig:?})");
        }
    }
    let items = startup_items();
    println!("startup items: {}", items.len());
    let t = Instant::now();
    startup_scan(&tx);
    println!("startup scan took {} ms", t.elapsed().as_millis());
    drop(tx);
    for msg in rx.try_iter() {
        if let Msg::Event(e) = msg {
            println!("  warning {} #{}: {:?}", e.channel, e.event_id, e.data);
        }
    }
    let _ = fresh;
    let defender = defender_status();
    match &defender {
        Some(d) => println!("defender: {}", serde_json::to_string(&d).unwrap_or_default()),
        None => println!("defender: status not available"),
    }
    let (tx, rx) = std::sync::mpsc::channel();
    let t = Instant::now();
    crate::checks::scan_all(&tx, &crate::checks::DriverCache::default());
    println!("persistence, network and driver checks took {} ms", t.elapsed().as_millis());
    drop(tx);
    for msg in rx.try_iter() {
        if let Msg::Event(e) = msg {
            println!("  warning {} #{}: {:?}", e.channel, e.event_id, e.data);
        }
    }
    let t = Instant::now();
    let report = crate::posture::report(defender);
    println!("security score {} ({} ms)", report.score, t.elapsed().as_millis());
    for c in &report.checks {
        println!("  [{}] {}: {}", c.status, c.title, c.detail);
    }
}

// --------------------------------------------------------------- Defender

pub(crate) fn defender_status() -> Option<DefenderInfo> {
    let windir = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    let ps = PathBuf::from(windir).join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
    let script = "$s = Get-MpComputerStatus -ErrorAction Stop; \
        $d = { param($t) if ($t -and $t.Year -gt 2000) { $t.ToString('yyyy-MM-ddTHH:mm:ss') } else { '' } }; \
        [pscustomobject]@{ antivirus = [bool]$s.AntivirusEnabled; realtime = [bool]$s.RealTimeProtectionEnabled; \
          tamper_protected = [bool]$s.IsTamperProtected; signature_age_days = [int]$s.AntivirusSignatureAge; \
          signatures_updated = (& $d $s.AntivirusSignatureLastUpdated); last_quick_scan = (& $d $s.QuickScanEndTime); \
          last_full_scan = (& $d $s.FullScanEndTime) } | ConvertTo-Json -Compress";
    let out = Command::new(ps)
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(0x0800_0000)
        .output()
        .ok()?;
    let mut info: DefenderInfo = serde_json::from_slice(String::from_utf8_lossy(&out.stdout).trim().as_bytes()).ok()?;
    info.checked_ms = now_ms();
    Some(info)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_scheduled_task_commands() {
        let xml = r#"<Task><Actions Context="Author"><Exec><Command>"C:\Users\a\AppData\Roaming\x\up.exe"</Command><Arguments>-w hidden &amp; go</Arguments></Exec><Exec><Command>C:\Windows\System32\cmd.exe</Command></Exec></Actions></Task>"#;
        assert_eq!(
            task_commands(xml),
            vec![
                (r#""C:\Users\a\AppData\Roaming\x\up.exe""#.to_string(), "-w hidden & go".to_string()),
                (r"C:\Windows\System32\cmd.exe".to_string(), String::new()),
            ]
        );
    }

    #[test]
    fn windows_folder_check() {
        let windir = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        assert!(in_windows_dir(&format!(r"{windir}\System32\svchost.exe")));
        assert!(in_windows_dir(&format!(r"{windir}\explorer.exe")));
        assert!(!in_windows_dir(r"C:\Users\a\AppData\Local\Temp\svchost.exe"));
        assert!(!in_windows_dir(&format!(r"{windir}\Temp\svchost.exe")));
    }

    #[test]
    fn utf16_task_files_decode() {
        let text = "<Exec><Command>a.exe</Command></Exec>";
        let mut bytes = vec![0xFF, 0xFE];
        bytes.extend(text.encode_utf16().flat_map(|w| w.to_le_bytes()));
        assert_eq!(decode_xml(&bytes), text);
    }
}
