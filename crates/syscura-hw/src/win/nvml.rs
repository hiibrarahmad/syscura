//! NVIDIA GPU sensors through NVML, the free monitoring library that ships
//! with every NVIDIA driver. Loaded on demand from System32 only, so a
//! planted DLL elsewhere on the search path is never picked up.

use std::ffi::{CStr, c_char, c_void};

use syscura_core::hw::{GpuInfo, Sensor, SensorKind, SensorSite};
use windows::Win32::Foundation::{FreeLibrary, HMODULE};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW};
use windows::core::{HSTRING, PCSTR};

type Device = *mut c_void;

#[repr(C)]
#[derive(Default)]
struct Utilization {
    gpu: u32,
    memory: u32,
}

struct Lib(HMODULE);

impl Drop for Lib {
    fn drop(&mut self) {
        unsafe {
            let _ = FreeLibrary(self.0);
        }
    }
}

impl Lib {
    /// Looks up a function. `F` must be the exact `extern "C"` signature.
    unsafe fn sym<F: Copy>(&self, name: &CStr) -> Option<F> {
        let p = unsafe { GetProcAddress(self.0, PCSTR(name.as_ptr() as *const u8)) }?;
        Some(unsafe { std::mem::transmute_copy(&p) })
    }
}

#[repr(C)]
#[derive(Default)]
struct MemInfo {
    total: u64,
    free: u64,
    used: u64,
}

/// Static facts NVML knows: VBIOS, PCIe link, power limit.
pub fn gpu_details(gpus: &mut [GpuInfo]) {
    if !gpus.iter().any(|g| g.vendor == "NVIDIA") {
        return;
    }
    let _ = unsafe { details(gpus) };
}

unsafe fn details(gpus: &mut [GpuInfo]) -> Option<()> {
    let module = unsafe { LoadLibraryExW(&HSTRING::from("nvml.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32) }.ok()?;
    let lib = Lib(module);
    let init: unsafe extern "C" fn() -> i32 = unsafe { lib.sym(c"nvmlInit_v2") }?;
    let shutdown: unsafe extern "C" fn() -> i32 = unsafe { lib.sym(c"nvmlShutdown") }?;
    let count: unsafe extern "C" fn(*mut u32) -> i32 = unsafe { lib.sym(c"nvmlDeviceGetCount_v2") }?;
    let handle: unsafe extern "C" fn(u32, *mut Device) -> i32 = unsafe { lib.sym(c"nvmlDeviceGetHandleByIndex_v2") }?;
    let vbios: Option<unsafe extern "C" fn(Device, *mut c_char, u32) -> i32> = unsafe { lib.sym(c"nvmlDeviceGetVbiosVersion") };
    let gen_now: Option<unsafe extern "C" fn(Device, *mut u32) -> i32> = unsafe { lib.sym(c"nvmlDeviceGetCurrPcieLinkGeneration") };
    let width_now: Option<unsafe extern "C" fn(Device, *mut u32) -> i32> = unsafe { lib.sym(c"nvmlDeviceGetCurrPcieLinkWidth") };
    let gen_max: Option<unsafe extern "C" fn(Device, *mut u32) -> i32> = unsafe { lib.sym(c"nvmlDeviceGetMaxPcieLinkGeneration") };
    let width_max: Option<unsafe extern "C" fn(Device, *mut u32) -> i32> = unsafe { lib.sym(c"nvmlDeviceGetMaxPcieLinkWidth") };
    let limit: Option<unsafe extern "C" fn(Device, *mut u32) -> i32> = unsafe { lib.sym(c"nvmlDeviceGetEnforcedPowerLimit") };
    if unsafe { init() } != 0 {
        return None;
    }
    let mut n = 0u32;
    if unsafe { count(&mut n) } == 0 {
        let nvidia: Vec<usize> = gpus.iter().enumerate().filter(|(_, g)| g.vendor == "NVIDIA").map(|(i, _)| i).collect();
        for (i, &idx) in nvidia.iter().enumerate().take(n as usize) {
            let mut dev: Device = std::ptr::null_mut();
            if unsafe { handle(i as u32, &mut dev) } != 0 {
                continue;
            }
            let g = &mut gpus[idx];
            let get = |f: Option<unsafe extern "C" fn(Device, *mut u32) -> i32>| {
                let mut v = 0u32;
                f.filter(|f| unsafe { f(dev, &mut v) } == 0).map(|_| v)
            };
            if let Some(f) = vbios {
                let mut buf = [0 as c_char; 64];
                if unsafe { f(dev, buf.as_mut_ptr(), buf.len() as u32) } == 0 {
                    g.vbios = unsafe { CStr::from_ptr(buf.as_ptr()) }.to_string_lossy().into_owned();
                }
            }
            let pcie = |gen_: Option<u32>, w: Option<u32>| match (gen_, w) {
                (Some(gen_), Some(w)) => Some(format!("{}.0 x{w}", gen_)),
                _ => None,
            };
            if let Some(now) = pcie(get(gen_now), get(width_now)) {
                g.pcie_link = match pcie(get(gen_max), get(width_max)) {
                    Some(max) => format!("PCIe {now} now (card supports {max}; it drops speed when idle to save power)"),
                    None => format!("PCIe {now}"),
                };
            }
            g.power_limit_w = get(limit).map(|mw| mw as f64 / 1000.0);
        }
    }
    unsafe { shutdown() };
    Some(())
}

pub fn gpu_sensors(gpus: &[GpuInfo]) -> Vec<Sensor> {
    if !gpus.iter().any(|g| g.vendor == "NVIDIA") {
        return Vec::new();
    }
    unsafe { read(gpus) }.unwrap_or_default()
}

unsafe fn read(gpus: &[GpuInfo]) -> Option<Vec<Sensor>> {
    let module = unsafe { LoadLibraryExW(&HSTRING::from("nvml.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32) }.ok()?;
    let lib = Lib(module);

    let init: unsafe extern "C" fn() -> i32 = unsafe { lib.sym(c"nvmlInit_v2") }?;
    let shutdown: unsafe extern "C" fn() -> i32 = unsafe { lib.sym(c"nvmlShutdown") }?;
    let count: unsafe extern "C" fn(*mut u32) -> i32 = unsafe { lib.sym(c"nvmlDeviceGetCount_v2") }?;
    let handle: unsafe extern "C" fn(u32, *mut Device) -> i32 =
        unsafe { lib.sym(c"nvmlDeviceGetHandleByIndex_v2") }?;
    let name: unsafe extern "C" fn(Device, *mut c_char, u32) -> i32 = unsafe { lib.sym(c"nvmlDeviceGetName") }?;
    let temp: unsafe extern "C" fn(Device, u32, *mut u32) -> i32 =
        unsafe { lib.sym(c"nvmlDeviceGetTemperature") }?;
    let fan: Option<unsafe extern "C" fn(Device, *mut u32) -> i32> = unsafe { lib.sym(c"nvmlDeviceGetFanSpeed") };
    let util: Option<unsafe extern "C" fn(Device, *mut Utilization) -> i32> =
        unsafe { lib.sym(c"nvmlDeviceGetUtilizationRates") };
    let power: Option<unsafe extern "C" fn(Device, *mut u32) -> i32> =
        unsafe { lib.sym(c"nvmlDeviceGetPowerUsage") };
    let clock: Option<unsafe extern "C" fn(Device, u32, *mut u32) -> i32> =
        unsafe { lib.sym(c"nvmlDeviceGetClockInfo") };
    let memory: Option<unsafe extern "C" fn(Device, *mut MemInfo) -> i32> =
        unsafe { lib.sym(c"nvmlDeviceGetMemoryInfo") };

    if unsafe { init() } != 0 {
        return None;
    }
    let mut out = Vec::new();
    let mut n = 0u32;
    if unsafe { count(&mut n) } == 0 {
        for i in 0..n {
            let mut dev: Device = std::ptr::null_mut();
            if unsafe { handle(i, &mut dev) } != 0 {
                continue;
            }
            let mut buf = [0 as c_char; 96];
            let dev_name = if unsafe { name(dev, buf.as_mut_ptr(), buf.len() as u32) } == 0 {
                unsafe { CStr::from_ptr(buf.as_ptr()) }.to_string_lossy().into_owned()
            } else {
                String::new()
            };
            // Attach to the matching inventory entry; fall back to the
            // n-th NVIDIA card.
            let index = gpus
                .iter()
                .position(|g| !dev_name.is_empty() && g.name.contains(&dev_name))
                .or_else(|| {
                    gpus.iter()
                        .enumerate()
                        .filter(|(_, g)| g.vendor == "NVIDIA")
                        .nth(i as usize)
                        .map(|(idx, _)| idx)
                })
                .unwrap_or(0);
            let site = SensorSite::Gpu(index);
            let mut push = |label: &str, kind, value: f64, unit: &str| {
                out.push(Sensor {
                    label: label.into(),
                    kind,
                    value: Some(value),
                    unit: unit.into(),
                    site: site.clone(),
                    source: "nvml".into(),
                });
            };
            let mut v = 0u32;
            if unsafe { temp(dev, 0, &mut v) } == 0 {
                push("GPU temperature", SensorKind::Temperature, v as f64, "°C");
            }
            if let Some(f) = fan
                && unsafe { f(dev, &mut v) } == 0
            {
                push("GPU fan", SensorKind::Fan, v as f64, "%");
            }
            if let Some(f) = util {
                let mut u = Utilization::default();
                if unsafe { f(dev, &mut u) } == 0 {
                    push("GPU load", SensorKind::Load, u.gpu as f64, "%");
                    push("GPU memory load", SensorKind::Load, u.memory as f64, "%");
                }
            }
            if let Some(f) = power
                && unsafe { f(dev, &mut v) } == 0
            {
                push("GPU power", SensorKind::Power, (v as f64 / 100.0).round() / 10.0, "W");
            }
            if let Some(f) = clock {
                // NVML_CLOCK_GRAPHICS = 0, NVML_CLOCK_MEM = 2
                if unsafe { f(dev, 0, &mut v) } == 0 {
                    push("GPU core clock", SensorKind::Clock, v as f64, "MHz");
                }
                if unsafe { f(dev, 2, &mut v) } == 0 {
                    push("GPU memory clock", SensorKind::Clock, v as f64, "MHz");
                }
            }
            if let Some(f) = memory {
                let mut m = MemInfo::default();
                if unsafe { f(dev, &mut m) } == 0 {
                    push("VRAM in use", SensorKind::Load, (m.used >> 20) as f64, "MB");
                }
            }
        }
    }
    unsafe { shutdown() };
    Some(out)
}
