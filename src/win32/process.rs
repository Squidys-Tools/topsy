//! Waiting on another process.

use std::time::Duration;

use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject};

use crate::log::LogErr;

/// Block until process `pid` has exited, or `timeout` passes. A process already gone returns
/// at once.
pub fn wait_for_exit(pid: u32, timeout: Duration) {
    let Ok(process) = (unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) }) else {
        return;
    };
    let millis = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX);
    unsafe {
        WaitForSingleObject(process, millis);
        CloseHandle(process).log_err();
    }
}
