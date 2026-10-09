//! `syscura-cli` — command-line client for the Syscura agent.
//!
//!   syscura-cli status              agent health and event counts
//!   syscura-cli events [-n N] [--errors] [--json]
//!   syscura-cli hw [--refresh] [--json]     hardware inventory
//!   syscura-cli problems [--all] [--json]   problems found and fixes tried
//!   syscura-cli fix <problem> <fix>         run a fix (numbers from `problems`)
//!   syscura-cli undo <attempt>              undo a fix that can be undone
//!   syscura-cli ignore <problem>            stop showing a problem
//!   syscura-cli processes [--warn] [--json] running programs (warnings only)
//!   syscura-cli security [--refresh] [--json] Windows' security settings, with a score
//!   syscura-cli summary [-d DAYS] [--json]  what happened lately (default 7 days)
//!   syscura-cli disks [--json]              daily drive health readings

use std::process::ExitCode;

use syscura_core::client::send;
use syscura_core::findings::{Finding, Harm, Risk};
use syscura_core::hw::HardwareInfo;
use syscura_core::{Level, Request, Response, StatusInfo, StoredEvent, now_ms};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let json = args.iter().any(|a| a == "--json");
    let request = match args.first().map(String::as_str) {
        Some("status") | None => Request::Status,
        Some("events") => {
            let limit = match args.iter().position(|a| a == "-n") {
                Some(i) => match args.get(i + 1).and_then(|n| n.parse().ok()) {
                    Some(n) => n,
                    None => return usage("-n needs a number"),
                },
                None => 20,
            };
            let max_level = args.iter().any(|a| a == "--errors").then_some(Level::Error);
            Request::Events { limit, max_level }
        }
        Some("hw") => Request::Hardware { refresh: args.iter().any(|a| a == "--refresh") },
        Some("actions") => Request::Actions,
        Some("processes") => Request::Processes,
        Some("problems") => Request::Findings { include_closed: args.iter().any(|a| a == "--all") },
        Some("security") => Request::Security { refresh: args.iter().any(|a| a == "--refresh") },
        Some("disks") => Request::DiskHistory,
        Some("summary") => {
            let days = match args.iter().position(|a| a == "-d") {
                Some(i) => match args.get(i + 1).and_then(|n| n.parse().ok()) {
                    Some(n) => n,
                    None => return usage("-d needs a number of days"),
                },
                None => 7,
            };
            Request::Summary { days }
        }
        Some(cmd @ ("fix" | "undo" | "ignore")) => {
            let num = |i: usize| args.get(i).and_then(|n| n.parse::<i64>().ok());
            match (cmd, num(1), num(2)) {
                ("fix", Some(finding), Some(fix)) => Request::Fix { finding, fix: fix as usize },
                ("undo", Some(attempt), _) => Request::Undo { attempt },
                ("ignore", Some(finding), _) => Request::Ignore { finding, ignore: true },
                _ => return usage("needs numbers from `syscura-cli problems`"),
            }
        }
        Some("help" | "-h" | "--help") => return usage(""),
        Some(other) => return usage(&format!("unknown command '{other}'")),
    };

    let response = match send(&request) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Cannot reach the Syscura agent: {e}");
            eprintln!("Start it with `syscura-agent run` or install the service with `syscura-agent install`.");
            return ExitCode::FAILURE;
        }
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&response).unwrap_or_default());
        return ExitCode::SUCCESS;
    }
    match response {
        Response::Status(s) => print_status(&s),
        Response::Events(events) => print_events(&events),
        Response::Hardware(hw) => print_hardware(&hw),
        Response::Findings(f) => print_findings(&f),
        Response::Done(msg) => println!("{msg}"),
        Response::Processes(list) => {
            let only_warnings = args.iter().any(|a| a == "--warn");
            println!("{:>7}  {:<28} {:>9} {:>6}  {:<10} PATH", "PID", "NAME", "MEMORY", "CPU", "SIGNATURE");
            for p in list.iter().filter(|p| !only_warnings || !p.warning.is_empty()) {
                println!("{:>7}  {:<28} {:>9} {:>5.1}%  {:<10} {}", p.pid, p.name, mb(p.memory_bytes), p.cpu_pct, p.signature, p.path);
                if !p.warning.is_empty() {
                    println!("         ! {}", p.warning);
                }
            }
        }
        Response::Actions(list) => {
            for a in list {
                println!("{:<26} {:?}  {}", a.id, a.risk, a.description);
            }
        }
        Response::Security(r) => {
            if r.checks.is_empty() {
                println!("The security check is running; ask again in a few seconds.");
            } else {
                println!("Security score: {} / 100{}", r.score, if r.refreshing { " (a new check is running)" } else { "" });
                for c in &r.checks {
                    println!("  [{:<7}] {}: {}", c.status, c.title, c.detail);
                }
            }
        }
        Response::Summary(s) => {
            println!("Last {} days:", s.days);
            println!("  new problems      {} ({} about security)", s.new_problems, s.security_problems);
            println!("  fixed             {} by Syscura, {} by you", s.fixed_automatically, s.fixed_by_you);
            println!("  still open        {}", s.open_problems);
            println!("  Windows events    {} errors, {} warnings", s.events.critical + s.events.error, s.events.warning);
            for h in &s.highlights {
                println!("  - {h}");
            }
        }
        Response::DiskHistory(points) => {
            println!("{:<12} {:<40} {:<10} {:>6} {:>6}", "DAY", "DRIVE", "HEALTH", "WEAR", "TEMP");
            for p in points {
                let wear = p.wear_pct.map(|w| format!("{w}%")).unwrap_or_else(|| "-".into());
                let temp = p.temperature_c.map(|t| format!("{t:.0}C")).unwrap_or_else(|| "-".into());
                println!("{:<12} {:<40} {:<10} {:>6} {:>6}", p.day, p.disk, p.health, wear, temp);
            }
        }
        Response::Options(o) => {
            for (k, v) in o {
                println!("{k} = {v}");
            }
        }
        Response::Error(e) => {
            eprintln!("Agent error: {e}");
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}

fn usage(problem: &str) -> ExitCode {
    if !problem.is_empty() {
        eprintln!("error: {problem}");
    }
    eprintln!(
        "usage: syscura-cli status [--json]
       syscura-cli events [-n N] [--errors] [--json]
       syscura-cli hw [--refresh] [--json]
       syscura-cli problems [--all] [--json]
       syscura-cli processes [--warn] [--json]
       syscura-cli security [--refresh] [--json]
       syscura-cli summary [-d DAYS] [--json]
       syscura-cli disks [--json]
       syscura-cli fix <problem> <fix> | undo <attempt> | ignore <problem>"
    );
    ExitCode::from(2)
}

fn mb(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
}

fn print_status(s: &StatusInfo) {
    let up = s.uptime_secs;
    println!("Syscura agent {} (pid {})", s.version, s.pid);
    println!("  uptime       {}h {:02}m {:02}s", up / 3600, up / 60 % 60, up % 60);
    println!("  memory       {} working set, {} private", mb(s.working_set_bytes), mb(s.private_bytes));
    println!("  sensors      {}", if s.sensors.is_empty() { "none".into() } else { s.sensors.join(", ") });
    println!("  events       {} stored", s.events_total);
    let c = &s.last_24h;
    println!(
        "  last 24h     {} critical, {} errors, {} warnings",
        c.critical, c.error, c.warning
    );
}

fn print_findings(findings: &[Finding]) {
    if findings.is_empty() {
        println!("No problems found.");
        return;
    }
    for f in findings {
        let harm = match f.harmful {
            Harm::Yes => "harmful",
            Harm::Maybe => "maybe harmful",
            Harm::No => "not harmful",
        };
        println!("#{:<4} [{}] {} ({}, {}x, {}, {})", f.id, f.severity.as_str(), f.title, f.category, f.count, harm, f.status.as_str());
        println!("       {}", f.explanation);
        for fix in &f.fixes {
            let risk = match fix.risk {
                Risk::Safe => "safe",
                Risk::Caution => "asks first",
                Risk::Risky => "risky",
            };
            println!("       fix {}: {} [{risk}]", fix.index, fix.label);
        }
        for a in f.attempts.iter().take(2) {
            let state = match (a.ok, a.verified) {
                (false, _) => "failed",
                _ if a.outcome == "nothing_found" => "found nothing wrong",
                _ if a.outcome == "not_repaired" => "not repaired",
                _ if a.outcome == "unclear" => "result unclear",
                (true, Some(true)) => "worked",
                (true, Some(false)) => "did not hold",
                (true, None) => "verifying",
            };
            let undo = if a.undo.is_some() && !a.undone { format!(" (undo: `syscura-cli undo {}`)", a.id) } else { String::new() };
            let who = if a.automatic { "automatically" } else { "by you" };
            println!("       tried {who}: {} -> {state}. {}{undo}", a.label, a.message);
        }
    }
}

fn print_hardware(hw: &HardwareInfo) {
    let gib = |b: u64| b as f64 / (1u64 << 30) as f64;
    println!("Board    {} {} (BIOS {} {})", hw.board.manufacturer, hw.board.product, hw.board.bios_version, hw.board.bios_date);
    for c in &hw.cpus {
        println!("CPU      {} ({} cores / {} threads, {})", c.name.trim(), c.cores, c.threads, c.socket);
    }
    for m in &hw.memory.sticks {
        println!("RAM      {:<8} {:.0} GB {} {} MT/s  {} {}", m.slot, gib(m.capacity_bytes), m.kind, m.configured_mts, m.manufacturer, m.part_number);
    }
    for s in &hw.slots {
        let state = match s.in_use { Some(true) => "in use", Some(false) => "empty", None => "?" };
        println!("Slot     {:<10} {}", s.name, state);
    }
    for g in &hw.gpus {
        println!("GPU      {} ({:.0} GB, {})", g.name, gib(g.vram_bytes), if g.board_partner.is_empty() { &g.vendor } else { &g.board_partner });
    }
    for d in &hw.disks {
        let temp = d.temperature_c.map(|t| format!(", {t}°C")).unwrap_or_default();
        println!("Drive    {} ({:.0} GB {} {}, {}{temp})", d.model, d.size_bytes as f64 / 1e9, d.media, d.bus, d.health);
    }
    for u in &hw.usb {
        println!("USB      {:<24} {:<15} panel: {:?}, port {} hub {}", u.name, u.kind, u.panel, u.port.unwrap_or(0), u.hub.unwrap_or(0));
    }
    for s in &hw.sensors {
        println!("Sensor   {:<22} {}", s.label, s.value.map(|v| format!("{v} {}", s.unit)).unwrap_or_else(|| "-".into()));
    }
    for n in &hw.notes {
        println!("Note     {n}");
    }
}

fn print_events(events: &[StoredEvent]) {
    if events.is_empty() {
        println!("No events recorded yet.");
        return;
    }
    let now = now_ms();
    for e in events {
        let e = &e.event;
        let age = ((now - e.ts) / 1000).max(0);
        let age = match age {
            a if a < 60 => format!("{a}s ago"),
            a if a < 3600 => format!("{}m ago", a / 60),
            a if a < 86400 => format!("{}h ago", a / 3600),
            a => format!("{}d ago", a / 86400),
        };
        let detail: Vec<String> = e.data.iter().filter(|(_, v)| !v.is_empty()).take(3)
            .map(|(k, v)| format!("{k}={}", truncate(v, 40)))
            .collect();
        println!(
            "{age:>8}  {:<8} {:<11} {:>5}  {}  {}",
            e.level.as_str(),
            e.channel,
            e.event_id,
            e.provider,
            detail.join(" ")
        );
    }
}

fn truncate(s: &str, max: usize) -> String {
    let s = s.replace(['\r', '\n'], " ");
    match s.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s,
    }
}
