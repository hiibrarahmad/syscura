//! The agent's own memory use, reported in `syscura status` so the
//! "tiny footprint" promise is visible and testable.

use windows::Win32::System::ProcessStatus::{
    K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
};
use windows::Win32::System::Threading::GetCurrentProcess;

/// Returns (working set, private bytes).
pub fn own_memory() -> (u64, u64) {
    let mut c = PROCESS_MEMORY_COUNTERS_EX {
        cb: size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        ..Default::default()
    };
    let ok = unsafe {
        K32GetProcessMemoryInfo(
            GetCurrentProcess(),
            &mut c as *mut _ as *mut PROCESS_MEMORY_COUNTERS,
            c.cb,
        )
    };
    if ok.as_bool() {
        (c.WorkingSetSize as u64, c.PrivateUsage as u64)
    } else {
        (0, 0)
    }
}
