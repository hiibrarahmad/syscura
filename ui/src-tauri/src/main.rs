//! Syscura desktop app. Talks to the background agent over its named pipe
//! and falls back to reading the hardware itself when the agent is not
//! running. Closing the window exits the app; nothing stays resident.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod images;

use std::sync::Mutex;

use serde::Serialize;
use syscura_core::hw::{HardwareInfo, Sensor};
use syscura_core::findings::Finding;
use syscura_core::{Level, Request, Response, StatusInfo, StoredEvent, client};
use tauri::{Manager, State};

use images::{ImageService, PartImage};

struct AppState {
    hardware: Mutex<Option<HardwareInfo>>,
    images: ImageService,
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

#[tauri::command]
async fn agent_status() -> Result<Option<StatusInfo>, String> {
    blocking(|| match client::send(&Request::Status) {
        Ok(Response::Status(s)) => Some(s),
        _ => None,
    })
    .await
}

#[tauri::command]
async fn events(limit: u32, errors_only: bool) -> Result<Vec<StoredEvent>, String> {
    let max_level = errors_only.then_some(Level::Error);
    blocking(move || match client::send(&Request::Events { limit, max_level }) {
        Ok(Response::Events(e)) => Ok(e),
        Ok(Response::Error(e)) => Err(e),
        Ok(_) => Err("unexpected reply from agent".into()),
        Err(_) => Err("agent_offline".into()),
    })
    .await?
}

#[tauri::command]
async fn hardware(state: State<'_, AppState>, refresh: bool) -> Result<HardwareView, String> {
    let view = blocking(move || match client::send(&Request::Hardware { refresh }) {
        Ok(Response::Hardware(hw)) => HardwareView { hw: *hw, from_agent: true },
        _ => HardwareView { hw: syscura_hw::collect(), from_agent: false },
    })
    .await?;
    state.images.set_parts(syscura_online::query::parts(&view.hw));
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

/// Sends a request to the agent and returns its reply text, or an error.
async fn agent_call(req: Request) -> Result<Response, String> {
    blocking(move || client::send(&req).map_err(|_| "agent_offline".to_string())).await?
}

#[tauri::command]
async fn findings(include_closed: bool) -> Result<Vec<Finding>, String> {
    match agent_call(Request::Findings { include_closed }).await? {
        Response::Findings(f) => Ok(f),
        Response::Error(e) => Err(e),
        _ => Err("unexpected reply from agent".into()),
    }
}

async fn done(req: Request) -> Result<String, String> {
    match agent_call(req).await? {
        Response::Done(m) => Ok(m),
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

/// Page to open first, from `--view <name>` on the command line.
#[tauri::command]
fn initial_view() -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter().position(|a| a == "--view").and_then(|i| args.get(i + 1).cloned())
}

/// `syscura-agent.exe` next to this app.
fn agent_exe() -> Result<std::path::PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let agent = exe.with_file_name("syscura-agent.exe");
    if agent.exists() { Ok(agent) } else { Err("syscura-agent.exe was not found next to Syscura.".into()) }
}

/// Starts background protection for this session (no admin rights).
#[tauri::command]
async fn start_agent() -> Result<String, String> {
    use std::os::windows::process::CommandExt;
    const DETACHED: u32 = 0x0000_0008 | 0x0800_0000; // DETACHED_PROCESS | CREATE_NO_WINDOW
    let agent = agent_exe()?;
    std::process::Command::new(agent)
        .arg("run")
        .creation_flags(DETACHED)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("could not start protection: {e}"))?;
    Ok("Protection started.".into())
}

/// Installs the agent as a Windows service (asks for admin rights once).
#[tauri::command]
async fn install_service() -> Result<String, String> {
    use std::os::windows::process::CommandExt;
    let agent = agent_exe()?;
    blocking(move || {
        // A console-mode agent holds the pipe; ask it to step aside first.
        let _ = client::send(&Request::Shutdown);
        std::thread::sleep(std::time::Duration::from_millis(800));
        let windir = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        let ps = std::path::Path::new(&windir).join(r"System32\WindowsPowerShell1.0\powershell.exe");
        // The path travels in an environment variable, never inside the script.
        let status = std::process::Command::new(ps)
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "$p = Start-Process -FilePath $env:SYSCURA_AGENT -ArgumentList 'install' -Verb RunAs -WindowStyle Hidden -Wait -PassThru; exit $p.ExitCode",
            ])
            .env("SYSCURA_AGENT", &agent)
            .creation_flags(0x0800_0000)
            .status()
            .map_err(|e| e.to_string())?;
        if status.success() {
            Ok("Syscura now runs as a Windows service and starts with Windows.".to_string())
        } else {
            Err("The service was not installed (the admin prompt was declined, or it is already installed).".to_string())
        }
    })
    .await?
}

#[tauri::command]
fn part_images(state: State<'_, AppState>) -> Vec<PartImage> {
    state.images.list()
}

#[tauri::command]
fn part_image_data(state: State<'_, AppState>, key: String) -> Option<String> {
    state.images.data_url(&key)
}

#[tauri::command]
fn retry_part_image(state: State<'_, AppState>, key: String) -> Result<(), String> {
    state.images.retry(&key)
}

#[tauri::command]
async fn set_part_image_url(app: tauri::AppHandle, key: String, url: String) -> Result<(), String> {
    blocking(move || app.state::<AppState>().images.set_from_url(&key, &url)).await?
}

#[tauri::command]
fn set_part_image_bytes(state: State<'_, AppState>, key: String, bytes: Vec<u8>, file_name: String) -> Result<(), String> {
    if bytes.len() > 8 * 1024 * 1024 {
        return Err("Pictures must be smaller than 8 MB.".into());
    }
    state.images.set_from_bytes(&key, &bytes, &file_name)
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let dir = app.path().app_local_data_dir()?.join("pictures");
            let images = ImageService::start(&dir)?;
            app.manage(AppState { hardware: Mutex::new(None), images });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            agent_status,
            events,
            hardware,
            live_sensors,
            part_images,
            part_image_data,
            retry_part_image,
            set_part_image_url,
            set_part_image_bytes,
            findings,
            run_fix,
            undo_fix,
            ignore_finding,
            start_agent,
            install_service,
            initial_view,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Syscura");
}
