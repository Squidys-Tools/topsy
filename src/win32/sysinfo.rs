//! The system's CPU times and physical memory, for the stats widget. Both are single cheap
//! kernel calls, fine on the UI thread. And Windows' build, for reports.

use windows::Win32::Foundation::FILETIME;
use windows::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW};
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows::Win32::System::Threading::GetSystemTimes;

use windows::core::w;

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

/// `24H2 (26100)`: Windows' version and build.
pub fn windows_build() -> Option<String> {
    let read = |name| {
        let mut buffer = [0u16; 64];
        let mut size = (buffer.len() * 2) as u32;
        // SAFETY: the buffer and its size in bytes go together.
        let got = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                w!(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion"),
                name,
                RRF_RT_REG_SZ,
                None,
                Some(buffer.as_mut_ptr().cast()),
                Some(&mut size),
            )
        };
        got.is_ok().then(|| {
            let text = &buffer[..(size as usize / 2).min(buffer.len())];
            String::from_utf16_lossy(text.split(|&c| c == 0).next().unwrap_or_default())
        })
    };
    let build = read(w!("CurrentBuild"))?;
    Some(match read(w!("DisplayVersion")) {
        Some(version) => format!("{version} ({build})"),
        None => build,
    })
}
