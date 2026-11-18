//! The battery: Windows calls back when the power source or the charge changes (and once with
//! the current state when asked), on a thread of its own. Nothing polls.

use std::sync::OnceLock;

use windows::Win32::Foundation::{ERROR_SUCCESS, HANDLE};
use windows::Win32::System::Power::{
    DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS, GetSystemPowerStatus, PowerSettingRegisterNotification,
    SYSTEM_POWER_STATUS,
};
use windows::Win32::System::SystemServices::{
    GUID_ACDC_POWER_SOURCE, GUID_BATTERY_PERCENTAGE_REMAINING,
};
use windows::Win32::UI::WindowsAndMessaging::DEVICE_NOTIFY_CALLBACK;

use model::hud::Power;

static REPORT: OnceLock<Box<dyn Fn(Power) + Send + Sync>> = OnceLock::new();
/// `BatteryFlag`: the machine has no battery.
const NO_BATTERY: u8 = 128;
/// `BatteryFlag`: not known yet.
const UNKNOWN: u8 = 255;

/// Report every change of the battery to `report`. A machine without one reports nothing.
pub fn watch(report: impl Fn(Power) + Send + Sync + 'static) {
    if REPORT.set(Box::new(report)).is_err() || !has_battery() {
        return;
    }
    // Windows keeps the pointer for as long as the registration lives: the life of the app.
    let params: &'static DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS =
        Box::leak(Box::new(DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS {
            Callback: Some(changed),
            Context: std::ptr::null_mut(),
        }));
    for setting in [GUID_ACDC_POWER_SOURCE, GUID_BATTERY_PERCENTAGE_REMAINING] {
        let mut handle = std::ptr::null_mut();
        // SAFETY: `params` lives for the rest of the process.
        let result = unsafe {
            PowerSettingRegisterNotification(
                &setting,
                DEVICE_NOTIFY_CALLBACK,
                HANDLE(params as *const _ as *mut _),
                &mut handle,
            )
        };
        if result != ERROR_SUCCESS {
            eprintln!("topsy: no battery notifications: {result:?}");
        }
    }
}

unsafe extern "system" fn changed(
    _context: *const core::ffi::c_void,
    _type: u32,
    _setting: *const core::ffi::c_void,
) -> u32 {
    if let (Some(report), Some(power)) = (REPORT.get(), read()) {
        report(power);
    }
    0
}

fn status() -> Option<SYSTEM_POWER_STATUS> {
    let mut status = SYSTEM_POWER_STATUS::default();
    // SAFETY: `status` is a valid out pointer.
    unsafe { GetSystemPowerStatus(&mut status) }.ok()?;
    Some(status)
}

/// Only a definite "no battery" counts; a battery whose state isn't known yet still gets watched.
fn has_battery() -> bool {
    status().is_none_or(|s| s.BatteryFlag == UNKNOWN || s.BatteryFlag & NO_BATTERY == 0)
}

fn read() -> Option<Power> {
    let status = status()?;
    let unknown = status.BatteryLifePercent > 100
        || status.BatteryFlag == UNKNOWN
        || status.ACLineStatus == UNKNOWN;
    (status.BatteryFlag & NO_BATTERY == 0 && !unknown).then_some(Power {
        percent: status.BatteryLifePercent,
        plugged: status.ACLineStatus == 1,
    })
}
