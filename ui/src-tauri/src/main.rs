//! Syscura desktop app. Talks to the background agent over its named pipe,
//! reads the hardware itself when the agent is not running, and talks to
//! the optional AI assistant with the user's own free key. Closing the
//! window exits the app; nothing stays resident.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod backup;
mod tray;
mod update;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use syscura_core::findings::{ActionInfo, Finding};
use syscura_core::hw::{HardwareInfo, Sensor};
use syscura_core::{Level, Request, Response, StatusInfo, StoredEvent, client};
use syscura_online::{Analysis, Gemini, Question, secrets};
use tauri::{Manager, State};

#[derive(Default, Serialize, Deserialize, Clone)]
#[serde(default)]
struct Settings {
    /// Model chosen when the key was saved (re-chosen if Google retires it).
    ai_model: Option<String>,
    /// Let the AI fix safe problems by itself.
    ai_auto_fix: bool,
}

struct AppState {
    /// The last update check: (when, result), so GitHub is asked at most
    /// every few hours.
    update: Mutex<Option<(std::time::Instant, update::UpdateInfo)>>,
    hardware: Mutex<Option<HardwareInfo>>,
    backup: Arc<Mutex<backup::Status>>,
    settings: Mutex<Settings>,
    settings_path: PathBuf,
}

impl AppState {
    fn settings(&self) -> Settings {
        self.settings.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn save(&self, s: Settings) {
        if let Ok(json) = serde_json::to_string_pretty(&s) {
            let _ = std::fs::write(&self.settings_path, json);
        }
        *self.settings.lock().unwrap_or_else(|e| e.into_inner()) = s;
    }
}

#[derive(Serialize)]
struct HardwareView {
    hw: HardwareInfo,
    /// True when the inventory came from the agent (with admin-only data).
    from_agent: bool,
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())
}

/// Sends a request to the agent; "agent_offline" when it is not running.
async fn agent_call(req: Request) -> Result<Response, String> {
    blocking(move || client::send(&req).map_err(|_| "agent_offline".to_string())).await?
}

async fn done(req: Request) -> Result<String, String> {
    match agent_call(req).await? {
        Response::Done(m) => Ok(m),
        Response::Error(e) => Err(e),
        _ => Err("unexpected reply from agent".into()),
    }
}

#[tauri::command]
async fn agent_status() -> Result<Option<StatusInfo>, String> {
    Ok(match agent_call(Request::Status).await {
        Ok(Response::Status(s)) => Some(s),
        _ => None,
    })
}

#[tauri::command]
async fn events(limit: u32, errors_only: bool) -> Result<Vec<StoredEvent>, String> {
    let max_level = errors_only.then_some(Level::Error);
    match agent_call(Request::Events { limit, max_level }).await? {
        Response::Events(e) => Ok(e),
        Response::Error(e) => Err(e),
        _ => Err("unexpected reply from agent".into()),
    }
}

/// Always a fresh read when asked to refresh; the agent's copy (which has
/// admin-only drive data) otherwise.
#[tauri::command]
async fn hardware(state: State<'_, AppState>, refresh: bool) -> Result<HardwareView, String> {
    let view = blocking(move || match client::send(&Request::Hardware { refresh }) {
        Ok(Response::Hardware(hw)) => HardwareView { hw: *hw, from_agent: true },
        _ => HardwareView { hw: syscura_hw::collect(), from_agent: false },
    })
    .await?;
    *state.hardware.lock().unwrap_or_else(|e| e.into_inner()) = Some(view.hw.clone());
    Ok(view)
}

#[tauri::command]
async fn live_sensors(state: State<'_, AppState>) -> Result<Vec<Sensor>, String> {
    let hw = state.hardware.lock().unwrap_or_else(|e| e.into_inner()).clone();
    match hw {
        Some(hw) => blocking(move || syscura_hw::live_sensors(&hw)).await,
        None => Ok(Vec::new()),
    }
}

#[tauri::command]
async fn findings(include_closed: bool) -> Result<Vec<Finding>, String> {
    match agent_call(Request::Findings { include_closed }).await? {
        Response::Findings(f) => Ok(f),
        Response::Error(e) => Err(e),
        _ => Err("unexpected reply from agent".into()),
    }
}

#[tauri::command]
async fn run_fix(finding: i64, fix: usize) -> Result<String, String> {
    done(Request::Fix { finding, fix }).await
}

#[tauri::command]
async fn undo_fix(attempt: i64) -> Result<String, String> {
    done(Request::Undo { attempt }).await
}

#[tauri::command]
async fn ignore_finding(finding: i64, ignore: bool) -> Result<String, String> {
    done(Request::Ignore { finding, ignore }).await
}

#[tauri::command]
async fn set_verdict(finding: i64, harmful: String, by: String) -> Result<String, String> {
    done(Request::SetVerdict { finding, harmful, by }).await
}

#[tauri::command]
async fn processes() -> Result<Vec<syscura_core::ProcessInfo>, String> {
    match agent_call(Request::Processes).await? {
        Response::Processes(p) => Ok(p),
        Response::Error(e) => Err(e),
        _ => Err("unexpected reply from agent".into()),
    }
}

/// Asks the AI to look up specifications Windows did not report, for this
/// exact PC. `device` names it (maker, model, board); `fields` are the
/// missing details as "Section > Detail".
#[tauri::command]
async fn hw_lookup(app: tauri::AppHandle, device: String, fields: Vec<String>) -> Result<syscura_online::SpecLookup, String> {
    let key = secrets::load_key().ok_or("Add your free Gemini key in Settings first.")?;
    let state = app.state::<AppState>();
    let model = state.settings().ai_model;
    blocking(move || Gemini::new(&key).lookup_specs(model.as_deref(), &device, &fields)).await?
}

/// The newest release on GitHub. Cached for 6 hours unless `force`.
pub async fn update_info(app: &tauri::AppHandle, force: bool) -> Result<update::UpdateInfo, String> {
    let state = app.state::<AppState>();
    if !force
        && let Some((when, info)) = state.update.lock().unwrap_or_else(|e| e.into_inner()).as_ref()
        && when.elapsed() < std::time::Duration::from_secs(6 * 3600)
    {
        return Ok(info.clone());
    }
    let info = blocking(update::check).await??;
    *state.update.lock().unwrap_or_else(|e| e.into_inner()) = Some((std::time::Instant::now(), info.clone()));
    Ok(info)
}

#[tauri::command]
async fn update_check(app: tauri::AppHandle, force: bool) -> Result<update::UpdateInfo, String> {
    update_info(&app, force).await
}

/// Downloads, verifies and starts the update, then closes Syscura so the
/// installer can replace it. The installer opens Syscura again.
#[tauri::command]
async fn update_install(app: tauri::AppHandle) -> Result<String, String> {
    let info = update_info(&app, true).await?;
    blocking(move || update::install(&info)).await??;
    let handle = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(1500));
        handle.exit(0);
    });
    Ok("Installing the update. Syscura closes now and opens again when it is done.".into())
}

#[tauri::command]
async fn recheck_files() -> Result<String, String> {
    done(Request::RecheckFiles).await
}

/// Which of these paths still exist.
#[tauri::command]
fn paths_exist(paths: Vec<String>) -> Vec<bool> {
    paths.iter().take(50).map(|p| std::path::Path::new(p).exists()).collect()
}

/// Folders that must never be deleted as a whole.
fn protected_folder(path: &str) -> bool {
    let p = path.trim_end_matches('\\').to_ascii_lowercase();
    let env = |k: &str| std::env::var(k).unwrap_or_default().trim_end_matches('\\').to_ascii_lowercase();
    let windir = env("SystemRoot");
    let profile = env("USERPROFILE");
    let users = std::path::Path::new(&profile).parent().map(|x| x.to_string_lossy().to_string()).unwrap_or_default();
    p.len() <= 3 // a drive root such as D:
        || p == windir || p.starts_with(&format!("{windir}\\"))
        || p == env("ProgramFiles") || p == env("ProgramFiles(x86)") || p == env("ProgramData")
        || p == profile || p == users
        || ["desktop", "documents", "downloads", "pictures", "music", "videos", "appdata"]
            .iter()
            .any(|f| p == format!("{profile}\\{f}"))
}

/// Moves a file, or a whole folder, to the Recycle Bin (it can be restored
/// from there). The app asks the person first; Windows' own folders and
/// other important folders are refused.
#[tauri::command]
async fn recycle_file(path: String) -> Result<String, String> {
    use windows::Win32::UI::Shell::{FO_DELETE, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, SHFILEOPSTRUCTW, SHFileOperationW};
    blocking(move || {
        let p = std::path::Path::new(&path);
        let b = path.as_bytes();
        let absolute = b.len() > 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'\\';
        if !absolute || path.chars().any(|c| c.is_control()) {
            return Err("That is not a full file path.".to_string());
        }
        if !p.exists() {
            return Err("It is no longer there: deleted already, or removed by Defender.".to_string());
        }
        if p.is_dir() && protected_folder(&path) {
            return Err("Syscura does not delete this folder: it is a drive or an important Windows or user folder.".to_string());
        }
        let windir = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into()).to_ascii_lowercase();
        if path.to_ascii_lowercase().starts_with(&format!("{windir}\\")) {
            return Err("Syscura does not delete files inside the Windows folder. Use \"Let Defender remove it\" instead.".to_string());
        }
        let mut from: Vec<u16> = path.encode_utf16().collect();
        from.extend([0, 0]); // double-null-terminated list
        let mut op = SHFILEOPSTRUCTW {
            wFunc: FO_DELETE,
            pFrom: windows::core::PCWSTR(from.as_ptr()),
            fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI).0 as u16,
            ..Default::default()
        };
        let r = unsafe { SHFileOperationW(&mut op) };
        if r != 0 || op.fAnyOperationsAborted.as_bool() || p.exists() {
            Err("Windows did not let Syscura move it (it may be in use, or Defender is holding it). Try \"Let Defender remove it\".".to_string())
        } else {
            // Close the problem right away if this was its last file.
            let _ = client::send(&Request::RecheckFiles);
            Ok("Moved to the Recycle Bin. You can restore it from there if this was a mistake.".to_string())
        }
    })
    .await?
}

#[tauri::command]
async fn actions() -> Result<Vec<ActionInfo>, String> {
    match agent_call(Request::Actions).await? {
        Response::Actions(a) => Ok(a),
        Response::Error(e) => Err(e),
        _ => Err("unexpected reply from agent".into()),
    }
}

/// Runs one catalog action. The agent re-checks it and refuses automatic
/// requests for anything that is not safe.
#[tauri::command]
async fn apply_action(
    finding: Option<i64>,
    title: String,
    action: String,
    params: BTreeMap<String, String>,
    label: String,
    automatic: bool,
) -> Result<String, String> {
    done(Request::ApplyAction { finding, title, action, params, label, automatic }).await
}

// ------------------------------------------------------------------ AI

#[derive(Serialize)]
struct AiStatus {
    configured: bool,
    model: Option<String>,
    auto_fix: bool,
}

#[tauri::command]
fn ai_status(state: State<'_, AppState>) -> AiStatus {
    let s = state.settings();
    AiStatus { configured: secrets::load_key().is_some(), model: s.ai_model, auto_fix: s.ai_auto_fix }
}

/// Checks the key with Google, picks the model, and stores the key in
/// Windows Credential Manager.
#[tauri::command]
async fn ai_save_key(app: tauri::AppHandle, key: String) -> Result<String, String> {
    let key = key.trim().to_string();
    if key.len() < 20 {
        return Err("That does not look like a Gemini API key.".into());
    }
    let model = blocking({
        let key = key.clone();
        move || Gemini::new(&key).verify()
    })
    .await??;
    secrets::save_key(&key)?;
    let state = app.state::<AppState>();
    let mut s = state.settings();
    s.ai_model = Some(model.clone());
    state.save(s);
    Ok(model)
}

#[tauri::command]
fn ai_forget_key(state: State<'_, AppState>) {
    secrets::delete_key();
    let mut s = state.settings();
    s.ai_model = None;
    state.save(s);
}

#[tauri::command]
fn ai_set_auto_fix(state: State<'_, AppState>, enabled: bool) {
    let mut s = state.settings();
    s.ai_auto_fix = enabled;
    state.save(s);
}

/// One line describing the PC, so answers fit this Windows version and
/// hardware. Contains no names, serials or addresses.
fn system_summary(hw: &HardwareInfo) -> String {
    let mut parts = Vec::new();
    let os = &hw.os;
    parts.push(format!("{} {} (build {}, {})", os.name, os.version, os.build, os.architecture));
    let sys = [hw.system.manufacturer.as_str(), hw.system.model.as_str()].join(" ");
    let board = [hw.board.manufacturer.as_str(), hw.board.product.as_str()].join(" ");
    parts.push(format!("{} PC: {} / board {} (BIOS {})", hw.system.chassis, sys.trim(), board.trim(), hw.board.bios_version));
    if let Some(vm) = &hw.system.virtual_machine {
        parts.push(format!("virtual machine: {vm}"));
    }
    for c in &hw.cpus {
        parts.push(format!("CPU {}", c.name.trim()));
    }
    let ram: u64 = hw.memory.sticks.iter().map(|m| m.capacity_bytes).sum();
    if ram > 0 {
        parts.push(format!("{} GB RAM", ram >> 30));
    }
    for g in &hw.gpus {
        parts.push(format!("GPU {} (driver {})", g.name, g.driver_version));
    }
    for d in &hw.disks {
        parts.push(format!("drive {} {} {} ({})", d.model, d.bus, d.media, d.health));
    }
    parts.join("; ")
}

#[tauri::command]
async fn ai_ask(app: tauri::AppHandle, mut question: Question) -> Result<Analysis, String> {
    let key = secrets::load_key().ok_or("Add your free Gemini key in Settings first.")?;
    let state = app.state::<AppState>();
    if question.system.is_empty()
        && let Some(hw) = state.hardware.lock().unwrap_or_else(|e| e.into_inner()).as_ref()
    {
        question.system = system_summary(hw);
    }
    // Without the agent there is no catalog, so the AI can only explain.
    let catalog = match agent_call(Request::Actions).await {
        Ok(Response::Actions(a)) => a,
        _ => Vec::new(),
    };
    let model = state.settings().ai_model;
    let result = blocking(move || {
        let gemini = Gemini::new(&key);
        let model = match model {
            Some(m) => m,
            None => gemini.verify()?,
        };
        // Falls back to other free models when one hits its limit.
        gemini.analyze_with_fallback(Some(&model), &question, &catalog)
    })
    .await??;
    if state.settings().ai_model.as_deref() != Some(result.model.as_str()) {
        let mut s = state.settings();
        s.ai_model = Some(result.model.clone());
        state.save(s);
    }
    Ok(result)
}

/// A plain, privacy-masked question for a free AI website the person opens
/// themselves (Gemini, ChatGPT, Claude, Copilot). No key needed.
#[tauri::command]
fn web_prompt(state: State<'_, AppState>, mut question: Question) -> String {
    if question.system.is_empty()
        && let Some(hw) = state.hardware.lock().unwrap_or_else(|e| e.into_inner()).as_ref()
    {
        question.system = system_summary(hw);
    }
    let s = syscura_online::Secrets::from_env();
    syscura_online::prompt::redact(&syscura_online::prompt::web_prompt(&question), &s)
}

// ------------------------------------------------------------------ backup

#[derive(Serialize)]
struct BackupInfo {
    folders: Vec<backup::Folder>,
    targets: Vec<backup::Target>,
    status: backup::Status,
}

#[tauri::command]
async fn backup_info(app: tauri::AppHandle) -> Result<BackupInfo, String> {
    let state = app.state::<AppState>();
    let volumes = state.hardware.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map(|h| h.volumes.clone()).unwrap_or_default();
    let status = state.backup.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let folders = blocking(backup::folders).await?;
    Ok(BackupInfo { folders, targets: backup::targets(&volumes), status })
}

#[tauri::command]
fn backup_start(state: State<'_, AppState>, destination: String, folders: Vec<String>) -> Result<String, String> {
    backup::start(state.backup.clone(), destination, folders)
}

// ------------------------------------------------------------------ agent

/// `syscura-agent.exe` next to this app.
fn agent_exe() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let agent = exe.with_file_name("syscura-agent.exe");
    if agent.exists() { Ok(agent) } else { Err("syscura-agent.exe was not found next to Syscura.".into()) }
}

/// Starts background protection for this session (no admin rights).
#[tauri::command]
async fn start_agent() -> Result<String, String> {
    use std::os::windows::process::CommandExt;
    const DETACHED: u32 = 0x0000_0008 | 0x0800_0000; // DETACHED_PROCESS | CREATE_NO_WINDOW
    // Already running (as the service or from an earlier start): nothing to do.
    if blocking(|| client::send(&Request::Status).is_ok()).await? {
        return Ok("Protection is already on.".into());
    }
    std::process::Command::new(agent_exe()?)
        .arg("run")
        .creation_flags(DETACHED)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("could not start protection: {e}"))?;
    Ok("Protection started.".into())
}

/// State of the Syscura Windows service, read from `sc query`
/// ("RUNNING", "STOPPED", ...), or None when it is not installed.
fn service_state() -> Option<String> {
    use std::os::windows::process::CommandExt;
    let windir = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    let out = std::process::Command::new(std::path::Path::new(&windir).join(r"System32\sc.exe"))
        .args(["query", "Syscura"])
        .creation_flags(0x0800_0000)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().find(|l| l.trim_start().starts_with("STATE"))?;
    line.split_whitespace().nth(3).map(str::to_string)
}

/// Installs the agent as a Windows service (asks for admin rights once).
#[tauri::command]
async fn install_service() -> Result<String, String> {
    use std::os::windows::process::CommandExt;
    let agent = agent_exe()?;
    blocking(move || {
        if service_state().as_deref() == Some("RUNNING") {
            return Ok("Syscura already runs as a Windows service.".to_string());
        }
        // A console-mode agent holds the pipe; ask it to step aside first.
        let _ = client::send(&Request::Shutdown);
        std::thread::sleep(std::time::Duration::from_millis(800));
        let windir = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        let ps = std::path::Path::new(&windir).join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
        // The path travels in an environment variable, never inside the script.
        // `install` replaces an existing service itself: one admin prompt.
        let script = "Start-Process -FilePath $env:SYSCURA_AGENT -ArgumentList 'install' -Verb RunAs -WindowStyle Hidden -Wait";
        let _ = std::process::Command::new(ps)
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .env("SYSCURA_AGENT", &agent)
            .creation_flags(0x0800_0000)
            .status()
            .map_err(|e| e.to_string())?;
        // Judge by what Windows reports, not by the helper's exit code.
        for _ in 0..20 {
            match service_state().as_deref() {
                Some("RUNNING") => return Ok("Done. Syscura now runs as a Windows service and starts with Windows.".to_string()),
                Some(_) => std::thread::sleep(std::time::Duration::from_millis(500)),
                None => break,
            }
        }
        match service_state() {
            Some(state) => Err(format!("The service was installed but is {}. Restart the PC, or open Services and start \"Syscura\".", state.to_lowercase())),
            None => {
                // Keep protection on for this session.
                let _ = std::process::Command::new(&agent)
                    .arg("run")
                    .creation_flags(0x0000_0008 | 0x0800_0000)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn();
                Err("The service was not installed: Windows' admin prompt was cancelled. Protection stays on until you sign out.".to_string())
            }
        }
    })
    .await?
}

/// Page to open first, from `--view <name>` on the command line.
#[tauri::command]
fn initial_view() -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter().position(|a| a == "--view").and_then(|i| args.get(i + 1).cloned())
}

fn main() {
    let app = tauri::Builder::default()
        // Starting Syscura again opens the running copy's window.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| tray::show_window(app)))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            let dir = app.path().app_local_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let settings_path = dir.join("settings.json");
            let settings = std::fs::read_to_string(&settings_path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or(Settings { ai_model: None, ai_auto_fix: true });
            app.manage(AppState {
                update: Mutex::new(None),
                hardware: Mutex::new(None),
                backup: Arc::new(Mutex::new(backup::Status::default())),
                settings: Mutex::new(settings),
                settings_path,
            });
            tray::create(app.handle())?;
            // `--tray` (used when Windows starts) stays in the tray.
            if !std::env::args().any(|a| a == "--tray") {
                tray::show_window(app.handle());
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            agent_status,
            events,
            hardware,
            live_sensors,
            findings,
            run_fix,
            undo_fix,
            ignore_finding,
            set_verdict,
            processes,
            recycle_file,
            recheck_files,
            hw_lookup,
            update_check,
            update_install,
            paths_exist,
            actions,
            apply_action,
            ai_status,
            ai_save_key,
            ai_forget_key,
            ai_set_auto_fix,
            ai_ask,
            web_prompt,
            backup_info,
            backup_start,
            start_agent,
            install_service,
            initial_view,
        ])
        .build(tauri::generate_context!())
        .expect("error while starting Syscura");
    app.run(|_app, event| {
        // Closing the window keeps Syscura in the tray; only "Quit" exits.
        if let tauri::RunEvent::ExitRequested { api, code: None, .. } = event {
            api.prevent_exit();
        }
    });
}
