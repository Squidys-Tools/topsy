//! The screen's brightness: a thread of its own waits on WMI's `WmiMonitorBrightnessEvent`,
//! which the display driver raises on each change (the brightness keys, the slider). The wait
//! blocks; nothing polls. Only displays whose driver reports brightness to WMI (a laptop's
//! own panel) have the event; elsewhere the query fails and the thread ends, or it waits for
//! good on an event that never comes, which costs nothing.

use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
    CoSetProxyBlanket, EOAC_NONE, RPC_C_AUTHN_LEVEL_CALL, RPC_C_IMP_LEVEL_IMPERSONATE,
};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::System::Wmi::{
    IEnumWbemClassObject, IWbemClassObject, IWbemLocator, WBEM_FLAG_FORWARD_ONLY,
    WBEM_FLAG_RETURN_IMMEDIATELY, WBEM_INFINITE, WbemLocator,
};
use windows::core::{BSTR, w};

/// `RPC_C_AUTHN_WINNT` and `RPC_C_AUTHZ_NONE`, kept here rather than pulling in the RPC bindings.
const AUTHN_WINNT: u32 = 10;
const AUTHZ_NONE: u32 = 0;

/// Report each change of brightness, in percent, to `report`.
pub fn watch(report: impl Fn(u8) + Send + 'static) {
    let spawned = std::thread::Builder::new()
        .name("topsy-brightness".into())
        .spawn(move || {
            let events = match unsafe { subscribe() } {
                Ok(events) => events,
                Err(err) => {
                    crate::log(&format!("no brightness events: {err}"));
                    return;
                }
            };
            loop {
                let mut found: [Option<IWbemClassObject>; 1] = [None];
                let mut returned = 0;
                // SAFETY: `found` and `returned` outlive the call.
                let waited = unsafe { events.Next(WBEM_INFINITE, &mut found, &mut returned) };
                if waited.is_err() {
                    eprintln!("topsy: brightness events stopped: {waited:?}");
                    return;
                }
                if let Some(percent) = found[0].as_ref().and_then(brightness) {
                    report(percent);
                }
            }
        });
    if let Err(err) = spawned {
        eprintln!("topsy: no brightness watcher: {err}");
    }
}

unsafe fn subscribe() -> windows::core::Result<IEnumWbemClassObject> {
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
        let locator: IWbemLocator = CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER)?;
        let none = BSTR::new();
        let services = locator.ConnectServer(
            &BSTR::from(r"ROOT\WMI"),
            &none,
            &none,
            &none,
            0,
            &none,
            None,
        )?;
        CoSetProxyBlanket(
            &services,
            AUTHN_WINNT,
            AUTHZ_NONE,
            None,
            RPC_C_AUTHN_LEVEL_CALL,
            RPC_C_IMP_LEVEL_IMPERSONATE,
            None,
            EOAC_NONE,
        )?;
        services.ExecNotificationQuery(
            &BSTR::from("WQL"),
            &BSTR::from("SELECT * FROM WmiMonitorBrightnessEvent"),
            WBEM_FLAG_RETURN_IMMEDIATELY | WBEM_FLAG_FORWARD_ONLY,
            None,
        )
    }
}

fn brightness(event: &IWbemClassObject) -> Option<u8> {
    let mut value = VARIANT::default();
    // SAFETY: `value` is a valid out pointer.
    unsafe { event.Get(w!("Brightness"), 0, &mut value, None, None) }.ok()?;
    u32::try_from(&value).ok().map(|v| v.min(100) as u8)
}
