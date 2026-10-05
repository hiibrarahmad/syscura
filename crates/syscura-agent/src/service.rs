//! Windows service plumbing: install/uninstall and the SCM entry point.

use std::ffi::OsString;
use std::sync::mpsc;
use std::time::Duration;

use windows_service::service::{
    ServiceAccess, ServiceAction, ServiceActionType, ServiceControl, ServiceControlAccept,
    ServiceErrorControl, ServiceExitCode, ServiceFailureActions, ServiceFailureResetPeriod,
    ServiceInfo, ServiceStartType, ServiceState, ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};
use windows_service::{define_windows_service, service_dispatcher};

use crate::agent::{self, Msg};
use crate::{default_data_dir, log};

const SERVICE_NAME: &str = "Syscura";
const DISPLAY_NAME: &str = "Syscura";
const DESCRIPTION: &str =
    "Watches Windows for software, hardware and security problems, and logs them.";

define_windows_service!(ffi_service_main, service_main);

/// Hands control to the Service Control Manager. Only works when Windows
/// started this process as a service.
pub fn dispatch() -> Result<(), String> {
    service_dispatcher::start(SERVICE_NAME, ffi_service_main)
        .map_err(|e| format!("not started by the Service Control Manager: {e}"))
}

fn service_main(_args: Vec<OsString>) {
    let data_dir = default_data_dir();
    log::init(&data_dir, false);
    if let Err(e) = run_service(data_dir) {
        log::error(&e);
    }
}

fn run_service(data_dir: std::path::PathBuf) -> Result<(), String> {
    let (tx, rx) = mpsc::channel();
    let stop_tx = tx.clone();
    let handler = move |control| match control {
        ServiceControl::Stop | ServiceControl::Shutdown => {
            let _ = stop_tx.send(Msg::Stop);
            ServiceControlHandlerResult::NoError
        }
        ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
        _ => ServiceControlHandlerResult::NotImplemented,
    };
    let status = service_control_handler::register(SERVICE_NAME, handler)
        .map_err(|e| format!("cannot register service handler: {e}"))?;

    let report = |state, exit: u32| {
        let _ = status.set_service_status(ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: state,
            controls_accepted: if state == ServiceState::Running {
                ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN
            } else {
                ServiceControlAccept::empty()
            },
            exit_code: ServiceExitCode::Win32(exit),
            checkpoint: 0,
            wait_hint: Duration::ZERO,
            process_id: None,
        });
    };

    report(ServiceState::Running, 0);
    let result = agent::run(data_dir, tx, rx, false);
    // ERROR_SERVICE_SPECIFIC_ERROR-style failure lets SCM recovery restart us.
    report(ServiceState::Stopped, if result.is_ok() { 0 } else { 1 });
    result
}

pub fn install() -> Result<(), String> {
    let manager = ServiceManager::local_computer(
        None::<&str>,
        ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE,
    )
    .map_err(|e| format!("cannot open the service manager (run as administrator): {e}"))?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let info = ServiceInfo {
        name: SERVICE_NAME.into(),
        display_name: DISPLAY_NAME.into(),
        service_type: ServiceType::OWN_PROCESS,
        start_type: ServiceStartType::AutoStart,
        error_control: ServiceErrorControl::Normal,
        executable_path: exe.clone(),
        launch_arguments: vec!["service".into()],
        dependencies: vec![],
        account_name: None, // LocalSystem
        account_password: None,
    };
    let service = manager
        .create_service(
            &info,
            ServiceAccess::CHANGE_CONFIG | ServiceAccess::START | ServiceAccess::QUERY_STATUS,
        )
        .map_err(|e| format!("cannot create service: {e}"))?;
    let _ = service.set_description(DESCRIPTION);
    // Restart after 5 s, then 30 s, then 2 min; forget failures after a day.
    let restart = |secs| ServiceAction {
        action_type: ServiceActionType::Restart,
        delay: Duration::from_secs(secs),
    };
    let _ = service.update_failure_actions(ServiceFailureActions {
        reset_period: ServiceFailureResetPeriod::After(Duration::from_secs(24 * 60 * 60)),
        reboot_msg: None,
        command: None,
        actions: Some(vec![restart(5), restart(30), restart(120)]),
    });
    service
        .start(&[] as &[&str])
        .map_err(|e| format!("service installed but did not start: {e}"))?;
    println!("Installed and started the '{DISPLAY_NAME}' service from {}", exe.display());
    Ok(())
}

pub fn uninstall() -> Result<(), String> {
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
        .map_err(|e| format!("cannot open the service manager (run as administrator): {e}"))?;
    let service = manager
        .open_service(
            SERVICE_NAME,
            ServiceAccess::QUERY_STATUS | ServiceAccess::STOP | ServiceAccess::DELETE,
        )
        .map_err(|e| format!("cannot open the Syscura service: {e}"))?;
    if service.query_status().is_ok_and(|s| s.current_state != ServiceState::Stopped) {
        let _ = service.stop();
        for _ in 0..50 {
            std::thread::sleep(Duration::from_millis(100));
            if service.query_status().is_ok_and(|s| s.current_state == ServiceState::Stopped) {
                break;
            }
        }
    }
    service.delete().map_err(|e| format!("cannot delete service: {e}"))?;
    println!("Removed the '{DISPLAY_NAME}' service. Data in {} was kept.", default_data_dir().display());
    Ok(())
}
