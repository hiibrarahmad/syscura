//! Hardware inventory and sensors.
//!
//! `collect()` is fairly heavy (WMI, COM): the always-on agent runs it in a
//! short-lived child process so the WMI libraries never stay loaded in it.
//! `live_sensors()` is cheap and meant for the UI to poll while it is open.

pub mod parse;

#[cfg(windows)]
mod win;

use syscura_core::hw::{HardwareInfo, Sensor};

/// Full inventory, including sensor readings available right now.
pub fn collect() -> HardwareInfo {
    #[cfg(windows)]
    {
        win::collect()
    }
    #[cfg(not(windows))]
    {
        HardwareInfo {
            collected_ms: syscura_core::now_ms(),
            notes: vec!["Hardware inventory is only implemented on Windows so far.".into()],
            ..Default::default()
        }
    }
}

/// Fast sensor readings (GPU via NVML, board/CPU via LibreHardwareMonitor).
/// `hw` is used to attach readings to the right parts.
pub fn live_sensors(hw: &HardwareInfo) -> Vec<Sensor> {
    #[cfg(windows)]
    {
        win::live_sensors(hw)
    }
    #[cfg(not(windows))]
    {
        let _ = hw;
        Vec::new()
    }
}
