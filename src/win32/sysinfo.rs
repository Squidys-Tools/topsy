//! The system's CPU times and physical memory, for the stats widget. Both are single cheap
//! kernel calls, fine on the UI thread.

use windows::Win32::Foundation::FILETIME;
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows::Win32::System::Threading::GetSystemTimes;

use model::stats::{CpuTimes, Memory};

/// Idle, kernel (idle included) and user time across all processors since boot.
pub fn cpu_times() -> windows::core::Result<CpuTimes> {
    let (mut idle, mut kernel, mut user) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    unsafe {
        GetSystemTimes(
            Some(&raw mut idle),
            Some(&raw mut kernel),
            Some(&raw mut user),
        )?
    };
    Ok(CpuTimes {
        idle: ticks(idle),
        kernel: ticks(kernel),
        user: ticks(user),
    })
}

pub fn memory() -> windows::core::Result<Memory> {
    let mut status = MEMORYSTATUSEX {
        dwLength: size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    unsafe { GlobalMemoryStatusEx(&mut status)? };
    Ok(Memory {
        total: status.ullTotalPhys,
        available: status.ullAvailPhys,
    })
}

fn ticks(time: FILETIME) -> u64 {
    (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime)
}
