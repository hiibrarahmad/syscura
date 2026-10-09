//! The tray icon. Syscura keeps running there when its window is closed:
//! the window (and its WebView) is destroyed to free memory, and a small
//! watcher keeps the tooltip current and sends a Windows notification for
//! each new serious problem.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use syscura_core::client;
use syscura_core::findings::{Finding, FindingStatus, Harm};
use syscura_core::{Level, Request, Response};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, WebviewWindowBuilder};
use tauri_plugin_notification::NotificationExt;

const TRAY_ID: &str = "syscura";

/// Opens the main window, or brings it to the front if it is open.
pub fn show_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    let Some(config) = app.config().app.windows.iter().find(|w| w.label == "main").cloned() else { return };
    match WebviewWindowBuilder::from_config(app, &config).and_then(|b| b.build()) {
        Ok(w) => {
            let _ = w.set_focus();
        }
        Err(e) => eprintln!("cannot open the Syscura window: {e}"),
    }
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Syscura", true, None::<&str>)?;
    let problems = MenuItem::with_id(app, "problems", "Show problems", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit (protection keeps running)", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &problems, &PredefinedMenuItem::separator(app)?, &quit])?;
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("Syscura")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_window(app),
            "problems" => {
                show_window(app);
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.eval("window.dispatchEvent(new CustomEvent('syscura-view', { detail: 'problems' }))");
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    start_watcher(app.clone());
    Ok(())
}

/// Problems where the person's files or PC are at real risk. Mirrors
/// `isCritical` in ui/src/lib/warnings.ts.
fn is_serious(f: &Finding) -> bool {
    if f.category == "noise" || f.harmful == Harm::No || !matches!(f.status, FindingStatus::Open | FindingStatus::FixFailed) {
        return false;
    }
    let dangerous = ["disk.errors", "disk.ntfs", "hw.whea_fatal", "sec.threat", "sys.bsod"];
    dangerous.contains(&f.rule_id.as_str()) || (f.harmful == Harm::Yes && matches!(f.severity, Level::Critical | Level::Error))
}

fn needs_attention(f: &Finding) -> bool {
    f.category != "noise"
        && matches!(f.status, FindingStatus::Open | FindingStatus::FixFailed)
        && !(f.harmful == Harm::No && (f.severity == Level::Info || f.verdict_by == "you"))
}

/// Once a day: is there a newer Syscura on GitHub? Notifies once per
/// version; installing happens from Settings (or the notification's app).
fn check_for_update(app: &AppHandle, marker: &Option<PathBuf>) {
    let Ok(info) = tauri::async_runtime::block_on(crate::update_info(app, true)) else { return };
    if !info.available {
        return;
    }
    let told = marker.as_ref().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
    if told.trim() == info.latest {
        return;
    }
    let _ = app
        .notification()
        .builder()
        .title(format!("Syscura {} is available", info.latest))
        .body("Open Syscura → Settings → Updates to install it. Your history and settings are kept.")
        .show();
    if let Some(p) = marker {
        let _ = std::fs::write(p, &info.latest);
    }
}

/// Once a week: what Syscura saw and did, as one notification.
fn weekly_summary(app: &AppHandle) {
    let state = app.state::<crate::AppState>();
    let mut settings = state.settings();
    let now = syscura_core::now_ms();
    if !settings.prefs.weekly_summary {
        return;
    }
    // The first week starts counting when Syscura first runs.
    if settings.last_summary_ms == 0 {
        settings.last_summary_ms = now;
        state.save(settings);
        return;
    }
    if now - settings.last_summary_ms < 7 * 86_400_000 {
        return;
    }
    let Ok(Response::Summary(s)) = client::send(&Request::Summary { days: 7 }) else { return };
    settings.last_summary_ms = now;
    state.save(settings);
    let fixed = s.fixed_automatically + s.fixed_by_you;
    let title = if s.new_problems == 0 {
        "Syscura's week: all quiet".to_string()
    } else {
        format!("Syscura's week: {} new problem{}, {fixed} fixed", s.new_problems, if s.new_problems == 1 { "" } else { "s" })
    };
    let mut body = format!(
        "{} errors and {} warnings in Windows' logs.",
        s.events.critical + s.events.error,
        s.events.warning
    );
    if s.security_problems > 0 {
        body.push_str(&format!(" {} security finding{}.", s.security_problems, if s.security_problems == 1 { "" } else { "s" }));
    }
    body.push_str(if s.open_problems > 0 { " Open Syscura to see what still needs you." } else { " Nothing needs you." });
    let _ = app.notification().builder().title(title).body(body).show();
}

/// Runs the scheduled backup when it is due and its drive is plugged in.
fn scheduled_backup(app: &AppHandle) {
    let state = app.state::<crate::AppState>();
    let mut settings = state.settings();
    let Some(mut schedule) = settings.prefs.backup_schedule.clone() else { return };
    let now = syscura_core::now_ms();
    if !schedule.due(now) || state.backup.lock().unwrap_or_else(|e| e.into_inner()).running {
        return;
    }
    if !std::path::Path::new(&schedule.destination).is_dir() {
        // Remind once a day while the drive stays unplugged past the due date.
        if now - schedule.last_ms >= (schedule.every_days as i64 + 1) * 86_400_000 && now % 86_400_000 < 30_000 {
            let _ = app
                .notification()
                .builder()
                .title("Syscura: backup waiting for its drive")
                .body(format!("Plug in {} and the backup starts by itself.", schedule.destination))
                .show();
        }
        return;
    }
    match crate::backup::start(state.backup.clone(), schedule.destination.clone(), schedule.folders.clone(), Some("Scheduled")) {
        Ok(dest) => {
            schedule.last_ms = now;
            settings.prefs.backup_schedule = Some(schedule);
            state.save(settings);
            let _ = app
                .notification()
                .builder()
                .title("Syscura: backing up your files")
                .body(format!("Copying changed files to {dest}. You can keep working."))
                .show();
        }
        Err(e) => eprintln!("scheduled backup did not start: {e}"),
    }
}

fn start_watcher(app: AppHandle) {
    let seen_path: Option<PathBuf> = app.path().app_local_data_dir().ok().map(|d| d.join("notified.json"));
    let update_marker: Option<PathBuf> = app.path().app_local_data_dir().ok().map(|d| d.join("update-notified.txt"));
    let mut last_update_check: Option<std::time::Instant> = None;
    let _ = std::thread::Builder::new().name("tray-watch".into()).spawn(move || {
        let mut seen: BTreeMap<i64, u64> = seen_path
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        loop {
            let tooltip = match client::send(&Request::Findings { include_closed: false }) {
                Ok(Response::Findings(list)) => {
                    let fresh: Vec<&Finding> = list.iter().filter(|f| is_serious(f) && seen.get(&f.id) != Some(&f.count)).collect();
                    if !fresh.is_empty() {
                        for f in &fresh {
                            seen.insert(f.id, f.count);
                        }
                        if let Some(p) = &seen_path {
                            let _ = std::fs::write(p, serde_json::to_string(&seen).unwrap_or_default());
                        }
                        let (title, body) = if fresh.len() == 1 {
                            (format!("Syscura: {}", fresh[0].title), "Open Syscura to see what it means and what to do.".to_string())
                        } else {
                            (format!("Syscura: {} serious problems", fresh.len()), "Back up your important files, then open Syscura for what to do.".to_string())
                        };
                        let _ = app.notification().builder().title(title).body(body).show();
                    }
                    let n = list.iter().filter(|f| needs_attention(f)).count();
                    match n {
                        0 => "Syscura: protection on, nothing needs you".to_string(),
                        1 => "Syscura: protection on, 1 thing needs you".to_string(),
                        n => format!("Syscura: protection on, {n} things need you"),
                    }
                }
                _ => "Syscura: protection is off. Open Syscura to start it.".to_string(),
            };
            if let Some(tray) = app.tray_by_id(TRAY_ID) {
                let _ = tray.set_tooltip(Some(tooltip));
            }
            // First check a few minutes after start, then daily.
            let due = match last_update_check {
                None => true,
                Some(t) => t.elapsed() >= Duration::from_secs(24 * 3600),
            };
            if due {
                last_update_check = Some(std::time::Instant::now());
                if app.state::<crate::AppState>().settings().prefs.auto_update_check {
                    check_for_update(&app, &update_marker);
                }
            }
            weekly_summary(&app);
            scheduled_backup(&app);
            std::thread::sleep(Duration::from_secs(30));
        }
    });
}
