//! Waiting on another process.

use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Threading::{
    INFINITE, OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
};

use crate::log::LogErr;

/// Block until process `pid` has exited. A process already gone returns at once.
pub fn wait_for_exit(pid: u32) {
    let Ok(process) = (unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) }) else {
        return;
    };
    unsafe {
        WaitForSingleObject(process, INFINITE);
        CloseHandle(process).log_err();
    }
}
