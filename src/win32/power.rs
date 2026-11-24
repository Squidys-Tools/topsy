//! Keeping the computer awake with a power request: Windows neither sleeps nor turns off the
//! display while one is held, and lists it in `powercfg /requests`.

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Power::{
    PowerCreateRequest, PowerRequestDisplayRequired, PowerRequestSystemRequired, PowerSetRequest,
};
use windows::Win32::System::SystemServices::POWER_REQUEST_CONTEXT_VERSION;
use windows::Win32::System::Threading::{
    POWER_REQUEST_CONTEXT_SIMPLE_STRING, REASON_CONTEXT, REASON_CONTEXT_0,
};
use windows::core::{PWSTR, w};

use crate::log::LogErr;

/// Held, the system and the display stay awake; dropped, they may sleep again. Unlike
/// `SetThreadExecutionState`, the request belongs to no thread.
pub struct KeepAwake(HANDLE);

impl KeepAwake {
    pub fn new() -> windows::core::Result<Self> {
        let context = REASON_CONTEXT {
            Version: POWER_REQUEST_CONTEXT_VERSION,
            Flags: POWER_REQUEST_CONTEXT_SIMPLE_STRING,
            Reason: REASON_CONTEXT_0 {
                // Windows only reads the reason, and copies it.
                SimpleReasonString: PWSTR(w!("topsy: Stay Awake").as_ptr().cast_mut()),
            },
        };
        // SAFETY: `context` and its static string outlive the call.
        let request = KeepAwake(unsafe { PowerCreateRequest(&context) }?);
        // SAFETY: a request handle of our own, open until drop.
        unsafe {
            PowerSetRequest(request.0, PowerRequestSystemRequired)?;
            PowerSetRequest(request.0, PowerRequestDisplayRequired)?;
        }
        Ok(request)
    }
}

impl Drop for KeepAwake {
    /// Closing the request clears whatever it set.
    fn drop(&mut self) {
        // SAFETY: the handle is ours and closed once.
        unsafe { CloseHandle(self.0) }.log_err();
    }
}
