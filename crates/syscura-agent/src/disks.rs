//! Drive health over time: one reading per drive per day (health, wear,
//! temperature, hours powered on), kept for a year. A drive rarely fails
//! without warning: wear that climbs fast, a health state that drops, or
//! heat are reported as problems ("Syscura/Disks").

use std::sync::mpsc::Sender;

use syscura_core::hw::HardwareInfo;
use syscura_core::{DiskPoint, Level};
use syscura_store::Store;

use crate::agent::Msg;
use crate::watch::emit;

/// Drive name as shown to people and stored in the history.
fn name(model: &str, device_id: &str) -> String {
    if device_id.is_empty() { model.trim().to_string() } else { format!("{} (disk {device_id})", model.trim()) }
}

pub fn today() -> String {
    let secs = syscura_core::now_ms() / 1000;
    let days = secs / 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}")
}

/// Records today's readings (once a day) and reports worrying changes.
pub fn record(store: &Store, hw: &HardwareInfo, tx: &Sender<Msg>) {
    let day = today();
    let history = store.disk_history(400).unwrap_or_default();
    if history.iter().any(|p| p.day == day) {
        return;
    }
    for d in &hw.disks {
        if d.model.trim().is_empty() {
            continue;
        }
        let point = DiskPoint {
            day: day.clone(),
            disk: name(&d.model, &d.device_id),
            health: if d.health.is_empty() { "Unknown".into() } else { d.health.clone() },
            temperature_c: d.temperature_c,
            wear_pct: d.wear_pct,
            power_on_hours: d.power_on_hours,
        };
        let past: Vec<&DiskPoint> = history.iter().filter(|p| p.disk == point.disk).collect();
        for (id, level, key, data) in judge(&point, &past) {
            emit(tx, "Syscura/Disks", id, level, &format!("{key}|{}|{day}", point.disk), [
                ("Disk", point.disk.clone()),
                ("Detail", data),
            ]);
        }
        if let Err(e) = store.add_disk_point(&point) {
            crate::log::error(&format!("cannot record drive health: {e}"));
        }
    }
}

/// (event id, level, kind, detail) for each worrying change. `past` is
/// oldest first.
pub fn judge(now: &DiskPoint, past: &[&DiskPoint]) -> Vec<(u32, Level, &'static str, String)> {
    let mut out = Vec::new();
    // Wear: compare with the oldest reading of the last 30 days.
    let month_ago = past.iter().rev().take(30).next_back();
    if let (Some(w), Some(old)) = (now.wear_pct, month_ago.and_then(|p| p.wear_pct))
        && w >= old + 5
    {
        out.push((50, Level::Warning, "wear", format!("Its wear rose from {old}% to {w}% in about a month, much faster than normal use.")));
    }
    if let Some(w) = now.wear_pct
        && w >= 90
        && !past.iter().any(|p| p.wear_pct.is_some_and(|x| x >= 90))
    {
        out.push((51, Level::Error, "worn", format!("It has used {w}% of its rated write life.")));
    }
    if let Some(t) = now.temperature_c
        && t >= 70.0
    {
        out.push((52, Level::Warning, "hot", format!("It was at {t:.0} °C when checked.")));
    }
    let rank = |h: &str| match h {
        "Healthy" => 0,
        "Warning" => 1,
        "Unhealthy" => 2,
        _ => -1,
    };
    if let Some(prev) = past.last()
        && rank(&prev.health) >= 0
        && rank(&now.health) > rank(&prev.health)
    {
        out.push((53, Level::Error, "health", format!("Windows' health state changed from {} to {}.", prev.health, now.health)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(day: &str, health: &str, wear: Option<u32>, temp: Option<f64>) -> DiskPoint {
        DiskPoint { day: day.into(), disk: "SSD".into(), health: health.into(), wear_pct: wear, temperature_c: temp, power_on_hours: None }
    }

    #[test]
    fn worrying_changes_are_found() {
        let a = p("2026-09-01", "Healthy", Some(10), Some(40.0));
        let b = p("2026-09-20", "Healthy", Some(12), Some(41.0));
        assert!(judge(&p("2026-10-01", "Healthy", Some(13), Some(45.0)), &[&a, &b]).is_empty(), "normal use is quiet");

        let ids = |v: Vec<(u32, Level, &str, String)>| v.into_iter().map(|x| x.0).collect::<Vec<_>>();
        assert_eq!(ids(judge(&p("2026-10-01", "Healthy", Some(16), None), &[&a, &b])), vec![50]);
        assert_eq!(ids(judge(&p("2026-10-01", "Warning", Some(12), Some(72.0)), &[&a, &b])), vec![52, 53]);
        assert_eq!(ids(judge(&p("2026-10-01", "Healthy", Some(91), None), &[])), vec![51]);
    }

    #[test]
    fn day_format() {
        let d = today();
        assert_eq!(d.len(), 10);
        assert_eq!(&d[4..5], "-");
    }
}
