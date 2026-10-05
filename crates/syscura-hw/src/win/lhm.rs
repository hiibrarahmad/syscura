//! Sensors from LibreHardwareMonitor (free, open source), when the user
//! runs it. It has the kernel driver needed for CPU and motherboard
//! sensor chips and publishes readings over WMI.

use serde::Deserialize;
use syscura_core::hw::{HardwareInfo, Sensor, SensorKind, SensorSite};
use wmi::WMIConnection;

use super::text;

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct LhmSensor {
    name: Option<String>,
    sensor_type: Option<String>,
    value: Option<f32>,
    identifier: Option<String>,
}

pub fn sensors(hw: &HardwareInfo, have_gpu: bool) -> Vec<Sensor> {
    let Ok(con) = WMIConnection::with_namespace_path(r"ROOT\LibreHardwareMonitor") else {
        return Vec::new();
    };
    let rows: Vec<LhmSensor> = con
        .raw_query("SELECT Name, SensorType, Value, Identifier FROM Sensor")
        .unwrap_or_default();
    rows.into_iter()
        .filter_map(|r| {
            let name = text(r.name);
            let id = text(r.identifier).to_ascii_lowercase();
            let (kind, unit) = match r.sensor_type.as_deref()? {
                "Temperature" => (SensorKind::Temperature, "°C"),
                "Fan" => (SensorKind::Fan, "RPM"),
                "Voltage" => (SensorKind::Voltage, "V"),
                "Load" => (SensorKind::Load, "%"),
                "Power" => (SensorKind::Power, "W"),
                _ => return None,
            };
            let site = site_for(&id, &name, kind, hw, have_gpu)?;
            let value = r.value.map(|v| (v as f64 * 10.0).round() / 10.0);
            Some(Sensor { label: name, kind, value, unit: unit.into(), site, source: "lhm".into() })
        })
        .collect()
}

/// Maps a LibreHardwareMonitor sensor to a place on the board. Returns
/// `None` for readings that are noise for this view or already covered.
fn site_for(id: &str, name: &str, kind: SensorKind, hw: &HardwareInfo, have_gpu: bool) -> Option<SensorSite> {
    let n = name.to_ascii_lowercase();
    if id.starts_with("/amdcpu") || id.starts_with("/intelcpu") {
        let keep = match kind {
            SensorKind::Temperature => n.contains("tctl") || n.contains("package") || n.contains("core (") || n == "core",
            SensorKind::Load => n.contains("total"),
            SensorKind::Power => n.contains("package"),
            _ => false,
        };
        return keep.then_some(SensorSite::Cpu);
    }
    if id.starts_with("/lpc/") {
        return Some(match kind {
            SensorKind::Fan => SensorSite::FanHeader(name.to_string()),
            SensorKind::Temperature if n.contains("vrm") || n.contains("vsoc") => SensorSite::Vrm,
            SensorKind::Temperature if n.contains("chipset") || n.contains("pch") => SensorSite::Chipset,
            SensorKind::Temperature if n == "cpu" || n.starts_with("cpu ") => SensorSite::Cpu,
            SensorKind::Voltage if !(n.contains("vcore") || n.contains("+12") || n.contains("+5") || n.contains("+3.3")) => {
                return None;
            }
            _ => SensorSite::Board,
        });
    }
    if id.starts_with("/gpu") && !have_gpu {
        let keep = matches!(kind, SensorKind::Temperature | SensorKind::Load | SensorKind::Fan | SensorKind::Power);
        return keep.then_some(SensorSite::Gpu(0));
    }
    if (id.starts_with("/nvme") || id.starts_with("/hdd") || id.starts_with("/ssd"))
        && kind == SensorKind::Temperature
        && !hw.elevated
    {
        // LHM numbers drives in the same order as Windows' disk list.
        let idx: usize = id.split('/').nth(2)?.parse().ok()?;
        return hw.disks.get(idx).map(|d| SensorSite::Disk(d.device_id.clone()));
    }
    None
}
