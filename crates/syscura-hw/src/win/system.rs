//! System-wide facts: Windows version, firmware, memory totals, displays,
//! drives' volumes, network, audio and battery.

use serde::Deserialize;
use syscura_core::hw::*;
use windows::Win32::System::Registry::{HKEY_LOCAL_MACHINE, REG_ROUTINE_FLAGS, RegGetValueW};
use windows::Win32::System::SystemInformation::{FIRMWARE_TYPE, FirmwareTypeBios, FirmwareTypeUefi, GetFirmwareType};
use windows::core::HSTRING;
use wmi::WMIConnection;

use super::{Num, query, reg_string, text};
use crate::parse;

const CURRENT_VERSION: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ComputerSystem {
    manufacturer: Option<String>,
    model: Option<String>,
    system_family: Option<String>,
    #[serde(rename = "SystemSKUNumber")]
    system_sku_number: Option<String>,
    #[serde(default)]
    total_physical_memory: Num,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Enclosure {
    chassis_types: Option<Vec<Num>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Os {
    #[serde(rename = "OSArchitecture")]
    os_architecture: Option<String>,
    last_boot_up_time: Option<String>,
}

pub fn system_and_os(con: &WMIConnection, hw: &mut HardwareInfo) {
    if let Some(c) = query::<ComputerSystem>(
        con,
        "SELECT Manufacturer, Model, SystemFamily, SystemSKUNumber, TotalPhysicalMemory FROM Win32_ComputerSystem",
    )
    .into_iter()
    .next()
    {
        hw.system.manufacturer = clean_oem(&text(c.manufacturer));
        hw.system.model = clean_oem(&text(c.model));
        hw.system.family = clean_oem(&text(c.system_family));
        hw.system.sku = clean_oem(&text(c.system_sku_number));
        hw.memory.usable_bytes = c.total_physical_memory.u64();
    }
    let chassis = query::<Enclosure>(con, "SELECT ChassisTypes FROM Win32_SystemEnclosure")
        .into_iter()
        .flat_map(|e| e.chassis_types.unwrap_or_default())
        .find_map(|n| n.0)
        .unwrap_or(0);
    hw.system.chassis = parse::chassis_name(chassis).into();
    hw.system.is_laptop = parse::is_laptop_chassis(chassis);
    hw.system.virtual_machine = parse::virtual_machine(&hw.system.manufacturer, &hw.system.model).map(str::to_string);

    let mut fw = FIRMWARE_TYPE::default();
    if unsafe { GetFirmwareType(&mut fw) }.is_ok() {
        hw.system.firmware = if fw == FirmwareTypeUefi {
            "UEFI".into()
        } else if fw == FirmwareTypeBios {
            "Legacy BIOS".into()
        } else {
            String::new()
        };
    }
    hw.system.secure_boot = reg_dword(r"SYSTEM\CurrentControlSet\Control\SecureBoot\State", "UEFISecureBootEnabled").map(|v| v == 1);

    // Windows version from the registry; Win32_OperatingSystem is only
    // used for what the registry does not have.
    let key = HSTRING::from(CURRENT_VERSION);
    let build = reg_string(&key, "CurrentBuild").unwrap_or_default();
    let ubr = reg_dword(CURRENT_VERSION, "UBR");
    hw.os.build = match ubr {
        Some(u) if !build.is_empty() => format!("{build}.{u}"),
        _ => build.clone(),
    };
    hw.os.name = parse::windows_name(&reg_string(&key, "ProductName").unwrap_or_default(), &build);
    hw.os.edition = reg_string(&key, "EditionID").unwrap_or_default();
    hw.os.version = reg_string(&key, "DisplayVersion").or_else(|| reg_string(&key, "ReleaseId")).unwrap_or_default();
    hw.os.installed = reg_dword(CURRENT_VERSION, "InstallDate").map(|t| parse::unix_date(t as i64)).unwrap_or_default();
    if let Some(o) = query::<Os>(con, "SELECT OSArchitecture, LastBootUpTime FROM Win32_OperatingSystem").into_iter().next() {
        hw.os.architecture = text(o.os_architecture);
        hw.os.last_boot_ms = o.last_boot_up_time.as_deref().and_then(parse::wmi_datetime_ms);
    }
}

/// Firmware placeholders such as "To Be Filled By O.E.M." mean "not set".
fn clean_oem(s: &str) -> String {
    if parse::is_placeholder(s) { String::new() } else { s.to_string() }
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct MemoryArray {
    #[serde(default)]
    memory_devices: Num,
    #[serde(default, rename = "Use")]
    usage: Num,
    /// Kilobytes.
    #[serde(default)]
    max_capacity_ex: Num,
}

pub fn memory_totals(con: &WMIConnection, hw: &mut HardwareInfo) {
    // Use = 3 is system memory (not flash or video memory arrays).
    for a in query::<MemoryArray>(con, "SELECT MemoryDevices, Use, MaxCapacityEx FROM Win32_PhysicalMemoryArray") {
        if a.usage.0 == Some(3) {
            hw.memory.total_slots += a.memory_devices.u32();
            hw.memory.max_bytes += a.max_capacity_ex.u64() * 1024;
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct MonitorId {
    #[serde(default)]
    user_friendly_name: Option<Vec<Num>>,
    #[serde(default)]
    manufacturer_name: Option<Vec<Num>>,
    #[serde(default)]
    year_of_manufacture: Num,
    #[serde(default)]
    active: Option<bool>,
}

/// Monitors from their own EDID data (root\wmi WmiMonitorID).
pub fn displays() -> Vec<DisplayInfo> {
    let Ok(con) = WMIConnection::with_namespace_path(r"ROOT\WMI") else { return Vec::new() };
    let chars = |v: Option<Vec<Num>>| -> String {
        v.unwrap_or_default()
            .into_iter()
            .filter_map(|n| n.0)
            .take_while(|&c| c != 0)
            .filter_map(|c| char::from_u32(c as u32))
            .collect::<String>()
            .trim()
            .to_string()
    };
    query::<MonitorId>(&con, "SELECT UserFriendlyName, ManufacturerName, YearOfManufacture, Active FROM WmiMonitorID")
        .into_iter()
        .map(|m| {
            let code = chars(m.manufacturer_name);
            DisplayInfo {
                manufacturer: parse::pnp_vendor(&code).map(str::to_string).unwrap_or(code),
                model: chars(m.user_friendly_name),
                year: m.year_of_manufacture.0.filter(|&y| y > 1990 && y < 2100).map(|y| y as u32),
                active: m.active.unwrap_or(true),
            }
        })
        .collect()
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct LogicalDisk {
    #[serde(rename = "DeviceID")]
    device_id: Option<String>,
    volume_name: Option<String>,
    file_system: Option<String>,
    #[serde(default)]
    size: Num,
    #[serde(default)]
    free_space: Num,
    #[serde(default)]
    drive_type: Num,
}

pub fn volumes(con: &WMIConnection) -> Vec<VolumeInfo> {
    // 2 = removable, 3 = local disk.
    query::<LogicalDisk>(
        con,
        "SELECT DeviceID, VolumeName, FileSystem, Size, FreeSpace, DriveType FROM Win32_LogicalDisk \
         WHERE DriveType = 2 OR DriveType = 3",
    )
    .into_iter()
    .filter(|d| d.size.u64() > 0)
    .map(|d| VolumeInfo {
        letter: text(d.device_id),
        label: text(d.volume_name),
        file_system: text(d.file_system),
        size_bytes: d.size.u64(),
        free_bytes: d.free_space.u64(),
        removable: d.drive_type.0 == Some(2),
    })
    .collect()
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct NetAdapter {
    name: Option<String>,
    #[serde(rename = "NetConnectionID")]
    net_connection_id: Option<String>,
    manufacturer: Option<String>,
    #[serde(default)]
    net_connection_status: Num,
    #[serde(default)]
    speed: Num,
}

pub fn network(con: &WMIConnection) -> Vec<NetworkAdapter> {
    query::<NetAdapter>(
        con,
        "SELECT Name, NetConnectionID, Manufacturer, NetConnectionStatus, Speed FROM Win32_NetworkAdapter \
         WHERE PhysicalAdapter = TRUE",
    )
    .into_iter()
    .map(|n| {
        let name = text(n.name);
        let connected = n.net_connection_status.0 == Some(2);
        NetworkAdapter {
            kind: parse::network_kind(&name).into(),
            connection: text(n.net_connection_id),
            manufacturer: text(n.manufacturer),
            connected,
            // Speed is meaningless (often 9223372036854775807) when disconnected.
            speed_mbps: n.speed.0.filter(|&s| connected && s > 0 && s < 1_000_000_000_000).map(|s| s / 1_000_000),
            name,
        }
    })
    .collect()
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct SoundDevice {
    name: Option<String>,
    manufacturer: Option<String>,
}

pub fn audio(con: &WMIConnection) -> Vec<AudioDevice> {
    let mut out: Vec<AudioDevice> = query::<SoundDevice>(con, "SELECT Name, Manufacturer FROM Win32_SoundDevice")
        .into_iter()
        .map(|s| AudioDevice { name: text(s.name), manufacturer: text(s.manufacturer) })
        .filter(|a| !a.name.is_empty())
        .collect();
    out.dedup_by(|a, b| a.name == b.name);
    out
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Win32Battery {
    name: Option<String>,
    #[serde(default)]
    estimated_charge_remaining: Num,
    #[serde(default)]
    battery_status: Num,
    #[serde(default)]
    chemistry: Num,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct BatteryStatic {
    #[serde(default)]
    designed_capacity: Num,
    manufacture_name: Option<String>,
    device_name: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct BatteryFull {
    #[serde(default)]
    full_charged_capacity: Num,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct BatteryCycles {
    #[serde(default)]
    cycle_count: Num,
}

pub fn batteries(con: &WMIConnection) -> Vec<BatteryInfo> {
    let basic: Vec<Win32Battery> =
        query(con, "SELECT Name, EstimatedChargeRemaining, BatteryStatus, Chemistry FROM Win32_Battery");
    if basic.is_empty() {
        return Vec::new();
    }
    // Capacity and cycles come from the battery's own data (root\wmi).
    let wmi = WMIConnection::with_namespace_path(r"ROOT\WMI").ok();
    let statics: Vec<BatteryStatic> = wmi
        .as_ref()
        .map(|w| query(w, "SELECT DesignedCapacity, ManufactureName, DeviceName FROM BatteryStaticData"))
        .unwrap_or_default();
    let fulls: Vec<BatteryFull> = wmi
        .as_ref()
        .map(|w| query(w, "SELECT FullChargedCapacity FROM BatteryFullChargedCapacity"))
        .unwrap_or_default();
    let cycles: Vec<BatteryCycles> =
        wmi.as_ref().map(|w| query(w, "SELECT CycleCount FROM BatteryCycleCount")).unwrap_or_default();

    basic
        .into_iter()
        .enumerate()
        .map(|(i, b)| {
            let st = statics.get(i);
            let design = st.and_then(|s| s.designed_capacity.0).filter(|&v| v > 0);
            let full = fulls.get(i).and_then(|f| f.full_charged_capacity.0).filter(|&v| v > 0);
            BatteryInfo {
                name: st.map(|s| text(s.device_name.clone())).filter(|s| !s.is_empty()).unwrap_or_else(|| text(b.name)),
                manufacturer: st.map(|s| text(s.manufacture_name.clone())).unwrap_or_default(),
                chemistry: parse::battery_chemistry(b.chemistry.u64()).into(),
                charge_pct: b.estimated_charge_remaining.0.filter(|&c| c <= 100).map(|c| c as u32),
                status: parse::battery_status(b.battery_status.u64()).into(),
                design_mwh: design,
                full_mwh: full,
                wear_pct: match (design, full) {
                    (Some(d), Some(f)) if f <= d * 2 => Some(((1.0 - f as f64 / d as f64) * 1000.0).round() / 10.0),
                    _ => None,
                },
                cycle_count: cycles.get(i).and_then(|c| c.cycle_count.0).filter(|&c| c > 0).map(|c| c as u32),
            }
        })
        .collect()
}

fn reg_dword(key: &str, value: &str) -> Option<u32> {
    const RRF_RT_REG_DWORD: REG_ROUTINE_FLAGS = REG_ROUTINE_FLAGS(0x10);
    let mut v = 0u32;
    let mut size = 4u32;
    let err = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            &HSTRING::from(key),
            &HSTRING::from(value),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut v as *mut u32 as *mut _),
            Some(&mut size),
        )
    };
    err.is_ok().then_some(v)
}
