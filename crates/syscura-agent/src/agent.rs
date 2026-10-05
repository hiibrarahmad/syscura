//! The agent's main loop: sensors push messages into one channel, and this
//! thread is the only writer of events and findings. It sleeps between
//! messages.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use syscura_core::findings::FindingStatus;
use syscura_core::{Event, now_ms};
use syscura_rules::Engine;
use syscura_sensors::eventlog::{self, Sink, Subscription};
use syscura_store::{Recorded, Store};

use crate::hardware::{self, HardwareCache};
use crate::heal::{self, Job};
use crate::{ipc, log, trust};

const PRUNE_EVERY: Duration = Duration::from_secs(6 * 60 * 60);
const RETENTION: Duration = Duration::from_secs(90 * 24 * 60 * 60);
/// How far back the first check looks.
const HISTORY_DAYS: u32 = 7;
const HISTORY_MAX_PER_CHANNEL: usize = 5000;
/// Virus scans started automatically are at least this far apart.
const AUTO_SCAN_GAP_MS: i64 = 6 * 60 * 60 * 1000;

pub enum Msg {
    Event(Event),
    /// Past events found at startup: recorded, but never auto-fixed.
    History(Vec<Event>),
    Stop,
}

/// What the IPC thread needs from the agent.
pub struct Shared {
    pub started: Instant,
    pub sensors: Vec<String>,
    pub hardware: Arc<HardwareCache>,
    pub jobs: Mutex<Sender<Job>>,
    /// Running in a console (not as the Windows service).
    pub console: bool,
}

/// Runs until a `Msg::Stop` arrives on `rx`. `tx` is handed to sensors.
pub fn run(data_dir: PathBuf, tx: Sender<Msg>, rx: Receiver<Msg>, console: bool) -> Result<(), String> {
    std::fs::create_dir_all(&data_dir)
        .map_err(|e| format!("cannot create {}: {e}", data_dir.display()))?;
    let db_path = data_dir.join("syscura.db");
    let store =
        Store::open(&db_path).map_err(|e| format!("cannot open {}: {e}", db_path.display()))?;
    let mut engine = Engine::builtin()?;
    let queries = engine.subscriptions();

    let mut subscriptions = Vec::new();
    let mut sensors = Vec::new();
    for (channel, query) in &queries {
        let tx = tx.clone();
        let sink: Sink = Box::new(move |e| {
            let _ = tx.send(Msg::Event(e));
        });
        match Subscription::new(channel, query, sink) {
            Ok(s) => {
                subscriptions.push(s);
                sensors.push(format!("eventlog/{channel}"));
            }
            // Some logs (Defender) need admin rights; the service has them.
            Err(e) => log::error(&format!("cannot watch the {channel} log: {e}")),
        }
    }

    let (jobs_tx, jobs_rx) = mpsc::channel::<Job>();
    heal::start(db_path.clone(), jobs_rx)?;

    let hardware_cache = Arc::new(HardwareCache::default());
    let shared = Arc::new(Shared {
        started: Instant::now(),
        sensors,
        hardware: hardware_cache.clone(),
        jobs: Mutex::new(jobs_tx.clone()),
        console,
    });
    ipc::start(db_path, shared)?;
    hardware::start_boot_check(data_dir.clone(), hardware_cache, tx.clone());
    start_history_check(queries, tx.clone());
    log::info(&format!("agent started, data in {}", data_dir.display()));

    let mut last_prune: Option<Instant> = None;
    let mut last_auto_scan: i64 = 0;
    loop {
        if last_prune.is_none_or(|t| t.elapsed() >= PRUNE_EVERY) {
            prune(&store);
            last_prune = Some(Instant::now());
        }
        match rx.recv_timeout(PRUNE_EVERY) {
            Ok(Msg::Event(e)) => {
                process(&store, &mut engine, &e, Some((&jobs_tx, &mut last_auto_scan)));
            }
            Ok(Msg::History(events)) => {
                let mut found = 0;
                for e in &events {
                    found += process(&store, &mut engine, e, None);
                }
                log::info(&format!("checked {} past events from the last {HISTORY_DAYS} days: {found} findings", events.len()));
            }
            Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {}
        }
    }

    drop(subscriptions);
    log::info("agent stopped");
    Ok(())
}

/// Stores the event, runs the rules on it, records findings and (for live
/// events) starts a safe fix. Returns how many new findings it created.
fn process(store: &Store, engine: &mut Engine, e: &Event, live: Option<(&Sender<Job>, &mut i64)>) -> usize {
    match store.insert_event(e) {
        Ok(true) => {}
        // Seen before (history overlapping live events): already handled.
        Ok(false) => return 0,
        Err(err) => {
            log::error(&format!("cannot store event: {err}"));
            return 0;
        }
    }
    let mut new = 0;
    let mut live = live;
    for hit in engine.evaluate(e) {
        let Some(rule) = engine.rule(&hit.rule).cloned() else { continue };
        let mut evidence = hit.evidence;
        if let Some(kind) = &rule.enrich {
            trust::enrich(kind, &mut evidence);
        }
        let recorded = match store.record_hit(&rule.id, &hit.group, e.ts, &evidence) {
            Ok(r) => r,
            Err(err) => {
                log::error(&format!("cannot record finding: {err}"));
                continue;
            }
        };
        let id = match recorded {
            Recorded::New(id) => {
                new += 1;
                log::info(&format!("new finding {id}: {} [{}]", rule.title, hit.group));
                id
            }
            Recorded::Returned(id) => {
                log::info(&format!("finding {id} came back after a fix: {}", rule.title));
                id
            }
            Recorded::Repeat(id) => id,
            Recorded::Ignored(_) => continue,
        };
        // Automatic safe fixes only for things happening now.
        let Some((jobs, last_auto_scan)) = live.as_mut() else { continue };
        let status = store.finding(id).ok().flatten().map(|f| f.status);
        if !matches!(status, Some(FindingStatus::Open | FindingStatus::FixFailed)) {
            continue;
        }
        if let Some(fix) = heal::auto_fix_choice(engine, store, id, &rule.id) {
            let is_scan = rule.fixes[fix].action.starts_with("defender.");
            if is_scan && now_ms() - **last_auto_scan < AUTO_SCAN_GAP_MS {
                continue;
            }
            if is_scan {
                **last_auto_scan = now_ms();
            }
            let _ = jobs.send(Job::Fix { finding: id, fix, automatic: true });
        }
    }
    new
}

/// Reads the last days of matching events once, on a helper thread.
fn start_history_check(queries: std::collections::BTreeMap<String, String>, tx: Sender<Msg>) {
    let spawned = std::thread::Builder::new().name("history".into()).spawn(move || {
        let mut all = Vec::new();
        for (channel, query) in &queries {
            match eventlog::history(channel, query, HISTORY_DAYS, HISTORY_MAX_PER_CHANNEL) {
                Ok(events) => all.extend(events),
                Err(e) => log::error(&format!("cannot read past events of {channel}: {e}")),
            }
        }
        // Oldest first, across all logs, so repeat counting is realistic.
        all.sort_by_key(|e| e.ts);
        let _ = tx.send(Msg::History(all));
    });
    if let Err(e) = spawned {
        log::error(&format!("cannot start the history check: {e}"));
    }
}

fn prune(store: &Store) {
    let cutoff = now_ms() - RETENTION.as_millis() as i64;
    match store.prune_before(cutoff) {
        Ok(0) => {}
        Ok(n) => log::info(&format!("pruned {n} events older than 90 days")),
        Err(e) => log::error(&format!("prune failed: {e}")),
    }
}
