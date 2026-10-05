//! Hardware and system inventory, shared by the agent, the CLI and the UI.
//!
//! Accuracy rule: every value comes from Windows or the device itself.
//! Anything a machine does not report stays empty (`""`, `None`) and the UI
//! shows "Not reported" — Syscura never guesses.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct HardwareInfo {
    pub collected_ms: i64,
    /// True when admin-only data (drive temperature and wear) was readable.
    pub elevated: bool,
    pub system: SystemInfo,
    pub os: OsInfo,
    pub board: BoardInfo,
    pub cpus: Vec<CpuInfo>,
    pub memory: MemoryInfo,
    pub slots: Vec<ExpansionSlot>,
    pub gpus: Vec<GpuInfo>,
    pub displays: Vec<DisplayInfo>,
    pub disks: Vec<DiskInfo>,
    pub volumes: Vec<VolumeInfo>,
    pub network: Vec<NetworkAdapter>,
    pub audio: Vec<AudioDevice>,
    pub battery: Vec<BatteryInfo>,
    pub usb: Vec<UsbDevice>,
    pub sensors: Vec<Sensor>,
    /// Plain-language notes about what could not be read, and why.
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SystemInfo {
    pub manufacturer: String,
    pub model: String,
    /// Product family / SKU as the maker reports them (often useful for laptops).
    pub family: String,
    pub sku: String,
    /// "desktop", "laptop", "all-in-one", ... from the SMBIOS chassis type.
    pub chassis: String,
    pub is_laptop: bool,
    /// Running inside a virtual machine (VMware, Hyper-V guest, VirtualBox, ...).
    pub virtual_machine: Option<String>,
    /// "UEFI" or "Legacy BIOS".
    pub firmware: String,
    pub secure_boot: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct OsInfo {
    /// "Windows 11 Pro" (corrected: Windows 11 still calls itself "Windows 10" in places).
    pub name: String,
    /// "Professional", "Core", ...
    pub edition: String,
    /// Feature update, e.g. "24H2".
    pub version: String,
    /// Full build, e.g. "26100.2314".
    pub build: String,
    pub architecture: String,
    /// YYYY-MM-DD.
    pub installed: String,
    /// When Windows last started, Unix ms.
    pub last_boot_ms: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct BoardInfo {
    pub manufacturer: String,
    pub product: String,
    pub version: String,
    pub bios_vendor: String,
    pub bios_version: String,
    /// YYYY-MM-DD when known.
    pub bios_date: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CpuInfo {
    pub name: String,
    pub manufacturer: String,
    pub socket: String,
    pub cores: u32,
    pub threads: u32,
    /// Base clock in MHz as the firmware reports it.
    pub base_mhz: u32,
    pub l2_kb: u32,
    pub l3_kb: u32,
    /// "AMD64 Family 25 Model 33 Stepping 0".
    pub family: String,
    /// Voltage the firmware reports (a fixed SMBIOS value, not a live reading).
    pub bios_voltage: Option<f64>,
    pub virtualization: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MemoryInfo {
    /// Memory Windows can use, in bytes.
    pub usable_bytes: u64,
    /// Number of memory slots (0 if not reported).
    pub total_slots: u32,
    /// Most memory the board supports, in bytes (0 if not reported).
    pub max_bytes: u64,
    pub sticks: Vec<MemoryStick>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MemoryStick {
    /// Slot name as the firmware reports it, e.g. "DIMM_A2" or "ChannelA-DIMM0".
    pub slot: String,
    pub bank: String,
    pub capacity_bytes: u64,
    /// Rated speed in MT/s.
    pub speed_mts: u32,
    /// Speed the system actually runs it at.
    pub configured_mts: u32,
    pub manufacturer: String,
    pub part_number: String,
    /// "DDR4", "DDR5", "LPDDR5", ...
    pub kind: String,
    /// Configured voltage in volts (SMBIOS; not a live reading).
    pub voltage: Option<f64>,
    pub ranks: Option<u32>,
    pub ecc: bool,
    /// "DIMM", "SODIMM", ...
    pub form: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ExpansionSlot {
    /// Name as reported by the firmware, e.g. "PCIEX16_1".
    pub name: String,
    pub in_use: Option<bool>,
    pub lanes: Option<u32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct GpuInfo {
    pub name: String,
    /// Chip maker: "NVIDIA", "AMD", "Intel".
    pub vendor: String,
    /// Card maker from the PCI subsystem ID, e.g. "Gigabyte".
    pub board_partner: String,
    pub vram_bytes: u64,
    pub driver_version: String,
    pub driver_date: String,
    pub resolution: String,
    pub refresh_hz: u32,
    pub pnp_id: String,
    pub pcie_path: String,
    pub vbios: String,
    pub pcie_link: String,
    pub power_limit_w: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DisplayInfo {
    pub manufacturer: String,
    pub model: String,
    pub year: Option<u32>,
    pub active: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DiskInfo {
    pub model: String,
    /// "SSD", "HDD" or "Unknown".
    pub media: String,
    /// "NVMe", "SATA", "USB", ...
    pub bus: String,
    pub size_bytes: u64,
    /// "Healthy", "Warning", "Unhealthy" or "Unknown".
    pub health: String,
    pub firmware: String,
    pub temperature_c: Option<f64>,
    /// Percentage of rated write endurance used (SSDs).
    pub wear_pct: Option<u32>,
    pub power_on_hours: Option<u64>,
    pub device_id: String,
    pub pcie_path: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct VolumeInfo {
    /// "C:".
    pub letter: String,
    pub label: String,
    pub file_system: String,
    pub size_bytes: u64,
    pub free_bytes: u64,
    pub removable: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct NetworkAdapter {
    pub name: String,
    /// Name in Network Connections, e.g. "Ethernet", "Wi-Fi".
    pub connection: String,
    /// "Ethernet", "Wi-Fi", "Bluetooth".
    pub kind: String,
    pub manufacturer: String,
    pub connected: bool,
    pub speed_mbps: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioDevice {
    pub name: String,
    pub manufacturer: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct BatteryInfo {
    pub name: String,
    pub manufacturer: String,
    pub chemistry: String,
    pub charge_pct: Option<u32>,
    /// "Charging", "On battery", "Fully charged", ...
    pub status: String,
    pub design_mwh: Option<u64>,
    pub full_mwh: Option<u64>,
    /// How much of the original capacity has been lost.
    pub wear_pct: Option<f64>,
    pub cycle_count: Option<u32>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Panel {
    Front,
    Back,
    Top,
    Bottom,
    Left,
    Right,
    Internal,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct UsbDevice {
    pub name: String,
    /// "Mouse", "Keyboard", "Storage", "Audio", "Camera", "Bluetooth", "Hub", "Other".
    pub kind: String,
    pub manufacturer: String,
    pub instance_id: String,
    /// Raw Windows location, e.g. "Port_#0004.Hub_#0003".
    pub location: String,
    pub port: Option<u32>,
    pub hub: Option<u32>,
    /// Only set when the firmware explicitly describes the port.
    pub panel: Panel,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SensorKind {
    #[default]
    Temperature,
    Fan,
    Voltage,
    Load,
    Power,
    Clock,
}

/// Which part a sensor belongs to.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "at", content = "id", rename_all = "snake_case")]
pub enum SensorSite {
    Cpu,
    Vrm,
    Chipset,
    /// Index into `HardwareInfo::gpus`.
    Gpu(usize),
    /// `DiskInfo::device_id`.
    Disk(String),
    /// Slot name.
    Memory(String),
    /// Fan header name.
    FanHeader(String),
    #[default]
    Board,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Sensor {
    pub label: String,
    pub kind: SensorKind,
    pub value: Option<f64>,
    /// "°C", "RPM", "V", "%", "W", "MHz", "MB".
    pub unit: String,
    pub site: SensorSite,
    /// Where the reading came from: "windows", "nvml", "storage", "acpi", "lhm".
    pub source: String,
}

impl HardwareInfo {
    /// A stable description of the installed parts (not sensors, not USB),
    /// used to notice when hardware is added, removed or swapped.
    pub fn fingerprint(&self) -> Vec<String> {
        let mut parts = vec![format!("board: {} {}", self.board.manufacturer, self.board.product)];
        for c in &self.cpus {
            parts.push(format!("cpu: {}", c.name));
        }
        for m in &self.memory.sticks {
            parts.push(format!(
                "ram {}: {} GB {} {}",
                m.slot,
                m.capacity_bytes >> 30,
                m.manufacturer,
                m.part_number
            ));
        }
        for g in &self.gpus {
            parts.push(format!("gpu: {}", g.name));
        }
        for d in &self.disks {
            parts.push(format!("disk: {} ({} GB, {})", d.model, d.size_bytes / 1_000_000_000, d.bus));
        }
        parts.sort();
        parts
    }
}

/// Lines present only in `old` (removed) and only in `new` (added).
pub fn diff_fingerprints(old: &[String], new: &[String]) -> (Vec<String>, Vec<String>) {
    let removed = old.iter().filter(|p| !new.contains(p)).cloned().collect();
    let added = new.iter().filter(|p| !old.contains(p)).cloned().collect();
    (removed, added)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_removed_ram_stick() {
        let mut hw = HardwareInfo::default();
        hw.board.product = "TUF".into();
        for slot in ["DIMM_A2", "DIMM_B2"] {
            hw.memory.sticks.push(MemoryStick {
                slot: slot.into(),
                capacity_bytes: 8 << 30,
                part_number: "F4-3600C16-8GVK".into(),
                ..Default::default()
            });
        }
        let before = hw.fingerprint();
        hw.memory.sticks.pop();
        let (removed, added) = diff_fingerprints(&before, &hw.fingerprint());
        assert_eq!(removed, vec!["ram DIMM_B2: 8 GB  F4-3600C16-8GVK".to_string()]);
        assert!(added.is_empty());
    }

    #[test]
    fn old_snapshots_still_load() {
        // A snapshot saved by an older version (fewer fields) must still parse.
        let old = r#"{"collected_ms":1,"board":{"product":"X","form_factor":"atx"},"cpus":[]}"#;
        let hw: HardwareInfo = serde_json::from_str(old).unwrap();
        assert_eq!(hw.board.product, "X");
    }

    #[test]
    fn sensor_site_serializes_for_the_ui() {
        let s = serde_json::to_string(&SensorSite::Memory("DIMM_A2".into())).unwrap();
        assert_eq!(s, r#"{"at":"memory","id":"DIMM_A2"}"#);
        assert_eq!(serde_json::to_string(&SensorSite::Cpu).unwrap(), r#"{"at":"cpu"}"#);
    }
}
