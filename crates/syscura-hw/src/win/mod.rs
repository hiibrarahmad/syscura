//! Windows inventory via WMI, CfgMgr32 and the registry.

mod lhm;
mod nvml;
mod usb;

use serde::{Deserialize, Deserializer};
use syscura_core::hw::*;
use syscura_core::now_ms;
use windows::Win32::System::Registry::{HKEY_LOCAL_MACHINE, REG_ROUTINE_FLAGS, RRF_RT_REG_QWORD, RegGetValueW};
use windows::core::HSTRING;
use wmi::WMIConnection;

use crate::parse;

/// A WMI number. WMI returns 64-bit integers as strings and some others as
/// signed values, so accept anything numeric-looking.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct Num(pub Option<u64>);

impl<'de> Deserialize<'de> for Num {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            U(u64),
            I(i64),
            F(f64),
            S(String),
            B(bool),
        }
        Ok(Num(match Option::<Raw>::deserialize(d)? {
            Some(Raw::U(u)) => Some(u),
            Some(Raw::I(i)) => u64::try_from(i).ok(),
            Some(Raw::F(f)) if f >= 0.0 => Some(f as u64),
            Some(Raw::S(s)) => s.trim().parse().ok(),
            Some(Raw::B(b)) => Some(b as u64),
            _ => None,
        }))
    }
}

impl Num {
    fn u32(self) -> u32 {
        self.0.unwrap_or(0).min(u32::MAX as u64) as u32
    }
    fn u64(self) -> u64 {
        self.0.unwrap_or(0)
    }
}

fn text(s: Option<String>) -> String {
    s.map(|s| s.trim().to_string()).unwrap_or_default()
}

fn query<T: for<'de> Deserialize<'de>>(con: &WMIConnection, wql: &str) -> Vec<T> {
    con.raw_query(wql).unwrap_or_default()
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ComputerSystem {
    manufacturer: Option<String>,
    model: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Enclosure {
    chassis_types: Option<Vec<Num>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Os {
    caption: Option<String>,
    build_number: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct BaseBoard {
    manufacturer: Option<String>,
    product: Option<String>,
    version: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Bios {
    manufacturer: Option<String>,
    #[serde(rename = "SMBIOSBIOSVersion")]
    smbios_bios_version: Option<String>,
    release_date: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Processor {
    name: Option<String>,
    manufacturer: Option<String>,
    socket_designation: Option<String>,
    #[serde(default)]
    number_of_cores: Num,
    #[serde(default)]
    number_of_logical_processors: Num,
    #[serde(default)]
    max_clock_speed: Num,
    #[serde(default)]
    l2_cache_size: Num,
    #[serde(default)]
    l3_cache_size: Num,
    caption: Option<String>,
    #[serde(default)]
    current_voltage: Num,
    #[serde(default)]
    virtualization_firmware_enabled: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct PhysicalMemory {
    device_locator: Option<String>,
    bank_label: Option<String>,
    #[serde(default)]
    capacity: Num,
    #[serde(default)]
    speed: Num,
    #[serde(default)]
    configured_clock_speed: Num,
    manufacturer: Option<String>,
    part_number: Option<String>,
    #[serde(default, rename = "SMBIOSMemoryType")]
    smbios_memory_type: Num,
    #[serde(default)]
    configured_voltage: Num,
    #[serde(default)]
    attributes: Num,
    #[serde(default)]
    data_width: Num,
    #[serde(default)]
    total_width: Num,
    #[serde(default)]
    form_factor: Num,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct MemoryArray {
    #[serde(default)]
    memory_devices: Num,
    #[serde(default, rename = "Use")]
    usage: Num,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct SystemSlot {
    slot_designation: Option<String>,
    #[serde(default)]
    current_usage: Num,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct VideoController {
    name: Option<String>,
    #[serde(default, rename = "AdapterRAM")]
    adapter_ram: Num,
    driver_version: Option<String>,
    driver_date: Option<String>,
    #[serde(default)]
    current_horizontal_resolution: Num,
    #[serde(default)]
    current_vertical_resolution: Num,
    #[serde(default)]
    current_refresh_rate: Num,
    #[serde(rename = "PNPDeviceID")]
    pnp_device_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct PhysicalDisk {
    device_id: Option<String>,
    friendly_name: Option<String>,
    #[serde(default)]
    media_type: Num,
    #[serde(default)]
    bus_type: Num,
    #[serde(default)]
    spindle_speed: Num,
    #[serde(default)]
    size: Num,
    #[serde(default)]
    health_status: Num,
    firmware_version: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ReliabilityCounter {
    device_id: Option<String>,
    #[serde(default)]
    temperature: Num,
    #[serde(default)]
    wear: Num,
    #[serde(default)]
    power_on_hours: Num,
}

pub fn collect() -> HardwareInfo {
    let mut hw = HardwareInfo { collected_ms: now_ms(), ..Default::default() };
    let con = match WMIConnection::new() {
        Ok(c) => c,
        Err(e) => {
            hw.notes.push(format!("Windows Management Instrumentation is unavailable: {e}"));
            return hw;
        }
    };

    let cs: Vec<ComputerSystem> = query(&con, "SELECT Manufacturer, Model FROM Win32_ComputerSystem");
    let chassis = query::<Enclosure>(&con, "SELECT ChassisTypes FROM Win32_SystemEnclosure")
        .into_iter()
        .flat_map(|e| e.chassis_types.unwrap_or_default())
        .find_map(|n| n.0)
        .unwrap_or(0);
    let os: Vec<Os> = query(&con, "SELECT Caption, BuildNumber FROM Win32_OperatingSystem");
    if let Some(c) = cs.into_iter().next() {
        hw.system.manufacturer = text(c.manufacturer);
        hw.system.model = text(c.model);
    }
    hw.system.chassis = parse::chassis_name(chassis).into();
    if let Some(o) = os.into_iter().next() {
        hw.system.os = format!("{} (build {})", text(o.caption), text(o.build_number));
    }

    if let Some(b) = query::<BaseBoard>(&con, "SELECT Manufacturer, Product, Version FROM Win32_BaseBoard")
        .into_iter()
        .next()
    {
        hw.board.manufacturer = text(b.manufacturer);
        hw.board.product = text(b.product);
        hw.board.version = text(b.version);
    }
    if let Some(b) = query::<Bios>(&con, "SELECT Manufacturer, SMBIOSBIOSVersion, ReleaseDate FROM Win32_BIOS")
        .into_iter()
        .next()
    {
        hw.board.bios_vendor = text(b.manufacturer);
        hw.board.bios_version = text(b.smbios_bios_version);
        hw.board.bios_date = parse::wmi_date(&text(b.release_date));
    }

    hw.cpus = query::<Processor>(
        &con,
        "SELECT Name, Manufacturer, SocketDesignation, NumberOfCores, NumberOfLogicalProcessors, \
         MaxClockSpeed, L2CacheSize, L3CacheSize, Caption, CurrentVoltage, \
         VirtualizationFirmwareEnabled FROM Win32_Processor",
    )
    .into_iter()
    .map(|p| CpuInfo {
        name: text(p.name),
        manufacturer: text(p.manufacturer),
        socket: text(p.socket_designation),
        cores: p.number_of_cores.u32(),
        threads: p.number_of_logical_processors.u32(),
        max_mhz: p.max_clock_speed.u32(),
        l2_kb: p.l2_cache_size.u32(),
        l3_kb: p.l3_cache_size.u32(),
        family: text(p.caption),
        // SMBIOS: bit 7 set means the low bits are tenths of a volt.
        bios_voltage: p.current_voltage.0.filter(|&v| v > 0).map(|v| if v & 0x80 != 0 { (v & 0x7f) as f64 / 10.0 } else { v as f64 / 10.0 }),
        virtualization: p.virtualization_firmware_enabled,
    })
    .collect();

    // Use = 3 is system memory (not flash or video memory arrays).
    hw.memory.total_slots = query::<MemoryArray>(&con, "SELECT MemoryDevices, Use FROM Win32_PhysicalMemoryArray")
        .into_iter()
        .filter(|a| a.usage.0 == Some(3))
        .map(|a| a.memory_devices.u32())
        .sum();
    hw.memory.sticks = query::<PhysicalMemory>(
        &con,
        "SELECT DeviceLocator, BankLabel, Capacity, Speed, ConfiguredClockSpeed, Manufacturer, \
         PartNumber, SMBIOSMemoryType, ConfiguredVoltage, Attributes, DataWidth, TotalWidth, \
         FormFactor FROM Win32_PhysicalMemory",
    )
    .into_iter()
    .map(|m| MemoryStick {
        slot: text(m.device_locator),
        bank: text(m.bank_label),
        capacity_bytes: m.capacity.u64(),
        speed_mts: m.speed.u32(),
        configured_mts: m.configured_clock_speed.u32(),
        manufacturer: text(m.manufacturer),
        part_number: text(m.part_number),
        kind: parse::memory_kind(m.smbios_memory_type.u64()).into(),
        voltage: m.configured_voltage.0.filter(|&mv| mv > 0).map(|mv| mv as f64 / 1000.0),
        ranks: m.attributes.0.filter(|&r| r > 0 && r <= 8).map(|r| r as u32),
        ecc: m.total_width.u64() > m.data_width.u64() && m.data_width.u64() > 0,
        form: parse::memory_form(m.form_factor.u64()).into(),
    })
    .collect();

    hw.slots = query::<SystemSlot>(&con, "SELECT SlotDesignation, CurrentUsage FROM Win32_SystemSlot")
        .into_iter()
        .map(|s| {
            let name = text(s.slot_designation);
            ExpansionSlot {
                lanes: parse::slot_lanes(&name),
                in_use: match s.current_usage.0 {
                    Some(3) => Some(false),
                    Some(4) => Some(true),
                    _ => None,
                },
                name,
            }
        })
        .collect();

    hw.gpus = query::<VideoController>(
        &con,
        "SELECT Name, AdapterRAM, DriverVersion, DriverDate, CurrentHorizontalResolution, \
         CurrentVerticalResolution, CurrentRefreshRate, PNPDeviceID FROM Win32_VideoController",
    )
    .into_iter()
    .map(|v| {
        let pnp_id = text(v.pnp_device_id);
        let name = text(v.name);
        let (vendor, partner) = parse::gpu_vendors(&pnp_id);
        let (w, h) = (v.current_horizontal_resolution.u32(), v.current_vertical_resolution.u32());
        GpuInfo {
            // AdapterRAM is a 32-bit field that caps at 4 GB; the driver's
            // registry entry has the real size.
            vram_bytes: gpu_vram_from_registry(&name).unwrap_or(v.adapter_ram.u64()),
            name,
            vendor: vendor.into(),
            board_partner: partner.into(),
            driver_version: text(v.driver_version),
            driver_date: parse::wmi_date(&text(v.driver_date)),
            resolution: if w > 0 { format!("{w}x{h}") } else { String::new() },
            refresh_hz: v.current_refresh_rate.u32(),
            pcie_path: usb::location_path(&pnp_id).unwrap_or_default(),
            pnp_id,
            ..Default::default()
        }
    })
    .collect();

    hw.board.form_factor = parse::guess_form_factor(
        &hw.board.product,
        hw.slots.iter().filter(|s| s.name.to_ascii_uppercase().contains("PCIE")).count(),
        hw.memory.total_slots,
        parse::is_laptop_chassis(chassis),
    );

    nvml::gpu_details(&mut hw.gpus);
    collect_disks(&mut hw, &con);
    hw.usb = usb::devices(&con, hw.system.chassis == "desktop");
    hw.sensors.extend(acpi_thermal());
    hw.sensors.extend(live_sensors(&hw));

    if !hw.elevated {
        hw.notes.push("Drive temperature and wear need administrator rights (the Syscura service has them).".into());
    }
    let has_cpu_temp = hw
        .sensors
        .iter()
        .any(|s| s.site == SensorSite::Cpu && s.kind == SensorKind::Temperature);
    if !has_cpu_temp {
        hw.notes.push(
            "Windows only shares CPU temperature and board voltages with programs that load a \
             kernel driver. Syscura shows CPU load and real clock speed from Windows instead."
                .into(),
        );
    }
    hw
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct DiskDrive {
    #[serde(default)]
    index: Num,
    #[serde(rename = "PNPDeviceID")]
    pnp_device_id: Option<String>,
}

fn collect_disks(hw: &mut HardwareInfo, cimv2: &WMIConnection) {
    // Disk number -> PCI path of its controller (to tell which M.2 slot).
    let controller_paths: Vec<(u64, String)> = query::<DiskDrive>(cimv2, "SELECT Index, PNPDeviceID FROM Win32_DiskDrive")
        .into_iter()
        .filter_map(|d| Some((d.index.0?, usb::parent_location_path(&text(d.pnp_device_id))?)))
        .collect();
    let Ok(con) = WMIConnection::with_namespace_path(r"ROOT\Microsoft\Windows\Storage") else {
        hw.notes.push("Storage information is unavailable.".into());
        return;
    };
    let counters: Vec<ReliabilityCounter> = con
        .raw_query("SELECT DeviceId, Temperature, Wear, PowerOnHours FROM MSFT_StorageReliabilityCounter")
        .unwrap_or_default();
    hw.elevated = !counters.is_empty();

    for d in query::<PhysicalDisk>(
        &con,
        "SELECT DeviceId, FriendlyName, MediaType, BusType, SpindleSpeed, Size, HealthStatus, \
         FirmwareVersion FROM MSFT_PhysicalDisk",
    ) {
        let device_id = text(d.device_id);
        let bus = parse::bus_name(d.bus_type.u64());
        let counter = counters.iter().find(|c| c.device_id.as_deref() == Some(device_id.as_str()));
        let temperature_c = counter.and_then(|c| c.temperature.0).filter(|&t| t > 0).map(|t| t as f64);
        let mut media = parse::media_name(d.media_type.u64(), d.spindle_speed.0, bus);
        if media == "Unknown" {
            media = match seek_penalty(&device_id) {
                Some(true) => "HDD",
                Some(false) => "SSD",
                None => parse::media_from_model(&text(d.friendly_name.clone())).unwrap_or("Unknown"),
            };
        }
        let disk = DiskInfo {
            model: text(d.friendly_name),
            media: media.into(),
            bus: bus.into(),
            size_bytes: d.size.u64(),
            health: parse::health_name(d.health_status.u64()).into(),
            firmware: text(d.firmware_version),
            temperature_c,
            wear_pct: counter.and_then(|c| c.wear.0).map(|w| w as u32),
            power_on_hours: counter.and_then(|c| c.power_on_hours.0),
            pcie_path: device_id
                .parse::<u64>()
                .ok()
                .and_then(|n| controller_paths.iter().find(|(i, _)| *i == n))
                .map(|(_, p)| p.clone())
                .filter(|p| p.starts_with("PCIROOT"))
                .unwrap_or_default(),
            device_id: device_id.clone(),
        };
        if let Some(t) = temperature_c {
            hw.sensors.push(Sensor {
                label: format!("{} temperature", disk.model),
                kind: SensorKind::Temperature,
                value: Some(t),
                unit: "°C".into(),
                site: SensorSite::Disk(device_id),
                source: "storage".into(),
            });
        }
        hw.disks.push(disk);
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ThermalZone {
    instance_name: Option<String>,
    #[serde(default)]
    current_temperature: Num,
}

/// ACPI thermal zones (admin only). Many boards report a fixed placeholder
/// value here, so values outside a plausible range are dropped.
fn acpi_thermal() -> Vec<Sensor> {
    let Ok(con) = WMIConnection::with_namespace_path(r"ROOT\WMI") else { return Vec::new() };
    query::<ThermalZone>(&con, "SELECT InstanceName, CurrentTemperature FROM MSAcpi_ThermalZoneTemperature")
        .into_iter()
        .filter_map(|z| {
            // Tenths of a kelvin.
            let c = z.current_temperature.0? as f64 / 10.0 - 273.15;
            (5.0..110.0).contains(&c).then(|| Sensor {
                label: format!("ACPI zone {}", text(z.instance_name).rsplit('\\').next().unwrap_or("")),
                kind: SensorKind::Temperature,
                value: Some((c * 10.0).round() / 10.0),
                unit: "°C".into(),
                site: SensorSite::Board,
                source: "acpi".into(),
            })
        })
        .collect()
}

pub fn live_sensors(hw: &HardwareInfo) -> Vec<Sensor> {
    let mut out = cpu_activity(hw);
    let gpu = nvml::gpu_sensors(&hw.gpus);
    let have_gpu = !gpu.is_empty();
    out.extend(gpu);
    out.extend(lhm::sensors(hw, have_gpu));
    out
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ProcessorPerf {
    #[serde(default)]
    percent_processor_utility: Num,
    #[serde(default)]
    percent_processor_performance: Num,
    #[serde(default)]
    processor_frequency: Num,
}

/// CPU load and real (boost-aware) clock from Windows' own performance
/// counters, the same numbers Task Manager shows. No admin, no driver.
fn cpu_activity(hw: &HardwareInfo) -> Vec<Sensor> {
    let Ok(con) = WMIConnection::new() else { return Vec::new() };
    let rows: Vec<ProcessorPerf> = query(
        &con,
        "SELECT PercentProcessorUtility, PercentProcessorPerformance, ProcessorFrequency \
         FROM Win32_PerfFormattedData_Counters_ProcessorInformation WHERE Name = '_Total'",
    );
    let Some(r) = rows.into_iter().next() else { return Vec::new() };
    let base = r.processor_frequency.0.filter(|&f| f > 0).unwrap_or(hw.cpus.first().map(|c| c.max_mhz as u64).unwrap_or(0));
    let mut out = Vec::new();
    let mut push = |label: &str, kind, value: f64, unit: &str| {
        out.push(Sensor { label: label.into(), kind, value: Some(value), unit: unit.into(), site: SensorSite::Cpu, source: "windows".into() });
    };
    if let Some(u) = r.percent_processor_utility.0 {
        push("CPU load", SensorKind::Load, u.min(100) as f64, "%");
    }
    if let Some(p) = r.percent_processor_performance.0.filter(|_| base > 0) {
        push("CPU clock (average)", SensorKind::Clock, (base * p / 100) as f64, "MHz");
    }
    out
}

/// Asks the drive whether random access is slow (spinning platters). This
/// is how Windows itself tells HDDs from SSDs, and it needs no admin rights.
fn seek_penalty(device_id: &str) -> Option<bool> {
    use windows::Win32::Foundation::{CloseHandle, GENERIC_ACCESS_RIGHTS};
    use windows::Win32::Storage::FileSystem::{CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING};
    use windows::Win32::System::IO::DeviceIoControl;
    use windows::Win32::System::Ioctl::{
        DEVICE_SEEK_PENALTY_DESCRIPTOR, IOCTL_STORAGE_QUERY_PROPERTY, PropertyStandardQuery,
        STORAGE_PROPERTY_QUERY, StorageDeviceSeekPenaltyProperty,
    };

    let n: u32 = device_id.parse().ok()?;
    let path = HSTRING::from(format!(r"\\.\PhysicalDrive{n}"));
    // Zero access rights: enough for this query, no admin needed.
    let handle = unsafe {
        CreateFileW(
            &path,
            GENERIC_ACCESS_RIGHTS(0).0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            None,
        )
    }
    .ok()?;
    let query = STORAGE_PROPERTY_QUERY {
        PropertyId: StorageDeviceSeekPenaltyProperty,
        QueryType: PropertyStandardQuery,
        ..Default::default()
    };
    let mut desc = DEVICE_SEEK_PENALTY_DESCRIPTOR::default();
    let mut returned = 0u32;
    let ok = unsafe {
        DeviceIoControl(
            handle,
            IOCTL_STORAGE_QUERY_PROPERTY,
            Some(&query as *const _ as *const _),
            size_of::<STORAGE_PROPERTY_QUERY>() as u32,
            Some(&mut desc as *mut _ as *mut _),
            size_of::<DEVICE_SEEK_PENALTY_DESCRIPTOR>() as u32,
            Some(&mut returned),
            None,
        )
    };
    unsafe {
        let _ = CloseHandle(handle);
    }
    ok.ok()?;
    Some(desc.IncursSeekPenalty)
}

/// Real VRAM size from the display driver's registry key.
fn gpu_vram_from_registry(name: &str) -> Option<u64> {
    const CLASS: &str = r"SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}";
    for i in 0..16 {
        let key = HSTRING::from(format!(r"{CLASS}\{i:04}"));
        let Some(desc) = reg_string(&key, "DriverDesc") else { continue };
        if desc.trim() != name {
            continue;
        }
        let mut value = 0u64;
        let mut size = size_of::<u64>() as u32;
        let err = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                &key,
                &HSTRING::from("HardwareInformation.qwMemorySize"),
                RRF_RT_REG_QWORD,
                None,
                Some(&mut value as *mut u64 as *mut _),
                Some(&mut size),
            )
        };
        return err.is_ok().then_some(value);
    }
    None
}

fn reg_string(key: &HSTRING, value: &str) -> Option<String> {
    const RRF_RT_REG_SZ: REG_ROUTINE_FLAGS = REG_ROUTINE_FLAGS(0x2);
    let mut buf = [0u16; 256];
    let mut size = (buf.len() * 2) as u32;
    let err = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key,
            &HSTRING::from(value),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as *mut _),
            Some(&mut size),
        )
    };
    if err.is_err() {
        return None;
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]))
}
