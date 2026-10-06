//! Syscura background agent.
//!
//! Usage:
//!   syscura-agent run [--data-dir DIR]   run in this console (Ctrl+C to stop)
//!   syscura-agent install                install and start the Windows service (admin)
//!   syscura-agent uninstall              stop and remove the Windows service (admin)
//!   syscura-agent service                entry point used by the Service Control Manager

mod actions;
mod agent;
mod hardware;
mod heal;
mod ipc;
mod log;
mod meminfo;
mod service;
mod trust;
mod watch;

use std::path::PathBuf;
use std::process::ExitCode;

fn default_data_dir() -> PathBuf {
    let base = std::env::var_os("ProgramData").unwrap_or_else(|| r"C:\ProgramData".into());
    PathBuf::from(base).join("Syscura")
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("run") => {
            let data_dir = match args.iter().position(|a| a == "--data-dir") {
                Some(i) => match args.get(i + 1) {
                    Some(d) => PathBuf::from(d),
                    None => return usage("--data-dir needs a folder"),
                },
                None => default_data_dir(),
            };
            log::init(&data_dir, true);
            // Ctrl+C ends the process; SQLite's WAL journal makes that safe.
            let (tx, rx) = std::sync::mpsc::channel();
            agent::run(data_dir, tx, rx, true)
        }
        // Internal: run by the agent as a short-lived child process so the
        // WMI/COM libraries never stay loaded in the long-running agent.
        Some("hwscan") => {
            let hw = syscura_hw::collect();
            println!("{}", serde_json::to_string(&hw).unwrap_or_default());
            Ok(())
        }
        Some("service") => service::dispatch(),
        Some("install") => service::install(),
        Some("selftest") => {
            watch::self_test();
            Ok(())
        }
        Some("uninstall") => service::uninstall(),
        _ => return usage(""),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            log::error(&e);
            ExitCode::FAILURE
        }
    }
}

fn usage(problem: &str) -> ExitCode {
    if !problem.is_empty() {
        eprintln!("error: {problem}");
    }
    eprintln!(
        "usage: syscura-agent <run [--data-dir DIR] | install | uninstall | service>"
    );
    ExitCode::from(2)
}
