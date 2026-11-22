//! Bluetooth audio coming and going. An `IMMNotificationClient` hears an output endpoint turn
//! active or go away and hands its id to a thread of its own, which asks Configuration Manager
//! whether the endpoint's device container is a Bluetooth one, for its name, and for its
//! battery where Windows knows it. While the device stays connected, a `DeviceWatcher` on its
//! container says when a node's battery changes. Nothing polls.

use std::collections::HashMap;
use std::sync::mpsc;

use crate::log::LogErr;
use crate::win32::bluetooth::{self, BATTERY_NAME, CONTAINER};
use model::headset::Identity;
use windows::Devices::Enumeration::{DeviceInformation, DeviceInformationKind, DeviceWatcher};
use windows::Foundation::TypedEventHandler;
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::Media::Audio::{
    DEVICE_STATE, DEVICE_STATE_ACTIVE, DEVICE_STATE_NOTPRESENT, DEVICE_STATE_UNPLUGGED, EDataFlow,
    ERole, IMMDeviceEnumerator, IMMEndpoint, IMMNotificationClient, IMMNotificationClient_Impl,
    MMDeviceEnumerator, eRender,
};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, STGM_READ,
};
use windows::Win32::System::Variant::VT_CLSID;
use windows::core::{GUID, HSTRING, Interface, PCWSTR, RuntimeType, implement};
use windows_collections::IIterable;

const INTERFACE_NAME: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0x026e516e_b814_414b_83cd_856d6fef4822),
    pid: 2,
};

/// One Bluetooth output device arriving, leaving, or its battery changing.
pub struct Seen {
    /// Its container id, the same for its every endpoint.
    pub container: String,
    /// Its name is never empty.
    pub identity: Identity,
    pub battery: Option<u8>,
    pub change: Change,
}

pub enum Change {
    Connected,
    Disconnected,
    /// Still connected, its battery no longer what it was.
    Battery {
        was: Option<u8>,
    },
}

enum Heard {
    /// An audio endpoint changed state.
    Endpoint(String, DEVICE_STATE),
    /// A node in this watched container was added, changed or removed.
    Container(String),
}

/// A connected device: the endpoint it was first heard on, and what its battery last was.
struct Watched {
    endpoint: String,
    battery: Option<u8>,
    watcher: Option<DeviceWatcher>,
}

#[implement(IMMNotificationClient)]
struct Client(mpsc::Sender<Heard>);

impl IMMNotificationClient_Impl for Client_Impl {
    fn OnDeviceStateChanged(&self, id: &PCWSTR, state: DEVICE_STATE) -> windows::core::Result<()> {
        // SAFETY: the id is valid for this call.
        if let Ok(id) = unsafe { id.to_string() } {
            self.0.send(Heard::Endpoint(id, state)).log_err();
        }
        Ok(())
    }
    fn OnDeviceAdded(&self, _: &PCWSTR) -> windows::core::Result<()> {
        Ok(())
    }
    fn OnDeviceRemoved(&self, _: &PCWSTR) -> windows::core::Result<()> {
        Ok(())
    }
    fn OnDefaultDeviceChanged(
        &self,
        _: EDataFlow,
        _: ERole,
        _: &PCWSTR,
    ) -> windows::core::Result<()> {
        Ok(())
    }
    fn OnPropertyValueChanged(&self, _: &PCWSTR, _: &PROPERTYKEY) -> windows::core::Result<()> {
        Ok(())
    }
}

pub fn watch(report: impl Fn(Seen) + Send + 'static) {
    let spawned = std::thread::Builder::new()
        .name("topsy-earbuds".into())
        .spawn(move || {
            let (tx, rx) = mpsc::channel();
            // SAFETY: COM for this thread; the client stays registered for the thread's life.
            let devices = unsafe {
                CoInitializeEx(None, COINIT_MULTITHREADED).ok().log_err();
                CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL)
                    .and_then(|devices| {
                        let client: IMMNotificationClient = Client(tx.clone()).into();
                        devices.RegisterEndpointNotificationCallback(&client)?;
                        Ok((devices, client))
                    })
            };
            let (devices, _client) = match devices {
                Ok(devices) => devices,
                Err(err) => return eprintln!("topsy: no audio device events: {err}"),
            };
            let mut watched: HashMap<String, Watched> = HashMap::new();
            for heard in rx {
                match heard {
                    Heard::Endpoint(id, state) => {
                        let change = match state {
                            DEVICE_STATE_ACTIVE => Change::Connected,
                            DEVICE_STATE_NOTPRESENT | DEVICE_STATE_UNPLUGGED => {
                                Change::Disconnected
                            }
                            _ => continue,
                        };
                        // SAFETY: COM calls on the thread that created `devices`.
                        let Some(seen) = (unsafe { look_up(&devices, &id, change) }) else {
                            continue;
                        };
                        match seen.change {
                            Change::Disconnected => {
                                if let Some(gone) = watched.remove(&seen.container)
                                    && let Some(watcher) = gone.watcher
                                {
                                    watcher.Stop().log_err();
                                }
                            }
                            Change::Connected if !watched.contains_key(&seen.container) => {
                                let watcher = watch_container(&seen.container, &tx).log_err();
                                let battery = seen.battery;
                                let endpoint = id;
                                let device = Watched {
                                    endpoint,
                                    battery,
                                    watcher,
                                };
                                watched.insert(seen.container.clone(), device);
                            }
                            Change::Connected | Change::Battery { .. } => {}
                        }
                        report(seen);
                    }
                    Heard::Container(container) => {
                        let Some(device) = watched.get_mut(&container) else {
                            continue;
                        };
                        // SAFETY: COM calls on the thread that created `devices`.
                        let was = device.battery;
                        if let Some(seen) =
                            unsafe { look_up(&devices, &device.endpoint, Change::Battery { was }) }
                            && seen.battery.is_some()
                            && seen.battery != was
                        {
                            device.battery = seen.battery;
                            report(seen);
                        }
                    }
                }
            }
        });
    if let Err(err) = spawned {
        eprintln!("topsy: no audio device events: {err}");
    }
}

/// Watch the device nodes in `container` for their battery: the first enumeration, and every
/// change Windows hears after, wake the thread to read it again.
fn watch_container(
    container: &str,
    wake: &mpsc::Sender<Heard>,
) -> windows::core::Result<DeviceWatcher> {
    let filter = HSTRING::from(format!("System.Devices.ContainerId:=\"{{{container}}}\""));
    let properties = IIterable::from(vec![HSTRING::from(BATTERY_NAME)]);
    let watcher = DeviceInformation::CreateWatcherWithKindAqsFilterAndAdditionalProperties(
        &filter,
        &properties,
        DeviceInformationKind::Device,
    )?;
    watcher.Added(&waker(wake, container))?;
    watcher.Updated(&waker(wake, container))?;
    watcher.Removed(&waker(wake, container))?;
    watcher.Start()?;
    Ok(watcher)
}

fn waker<A: RuntimeType + 'static, B: RuntimeType + 'static>(
    wake: &mpsc::Sender<Heard>,
    container: &str,
) -> TypedEventHandler<A, B> {
    let (wake, container) = (wake.clone(), container.to_owned());
    TypedEventHandler::new(move |_, _| {
        wake.send(Heard::Container(container.clone())).log_err();
        Ok(())
    })
}

/// The endpoint `id` as a Bluetooth output, or `None` if it isn't one.
unsafe fn look_up(devices: &IMMDeviceEnumerator, id: &str, change: Change) -> Option<Seen> {
    unsafe {
        let device = devices.GetDevice(&HSTRING::from(id)).ok()?;
        if device.cast::<IMMEndpoint>().ok()?.GetDataFlow().ok()? != eRender {
            return None;
        }
        let props = device.OpenPropertyStore(STGM_READ).ok()?;
        let key = PROPERTYKEY {
            fmtid: CONTAINER,
            pid: 2,
        };
        let value = props.GetValue(&key).ok()?;
        if value.vt() != VT_CLSID {
            return None;
        }
        let container = *value.Anonymous.Anonymous.Anonymous.puuid;
        let endpoint_name = props
            .GetValue(&INTERFACE_NAME)
            .ok()
            .map(|v| v.to_string())
            .filter(|n| !n.is_empty());
        let mut found = bluetooth::in_container(container)?;
        if found.identity.name.is_empty() {
            found.identity.name = endpoint_name?;
        }
        Some(Seen {
            container: format!("{container:?}"),
            identity: found.identity,
            battery: found.battery,
            change,
        })
    }
}
