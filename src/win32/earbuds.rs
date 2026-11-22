//! Bluetooth audio coming and going. An `IMMNotificationClient` hears an output endpoint turn
//! active or go away and hands its id to a thread of its own, which asks Configuration Manager
//! whether the endpoint's device container is a Bluetooth one, for its name, and for its
//! battery where Windows knows it. While the device stays connected, a `DeviceWatcher` on its
//! container says when a node's battery changes. AirPods and Beats also get a few seconds of
//! listening for their own broadcasts as they connect and when Windows' battery moves, for each
//! bud's and the case's battery. Nothing polls.

use std::collections::HashMap;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::log::LogErr;
use crate::win32::adverts;
use crate::win32::bluetooth::{self, BATTERY_NAME, CONTAINER};
use model::headset::apple::{self, Scan};
use model::headset::{Identity, Power};
use windows::Devices::Bluetooth::Advertisement::BluetoothLEAdvertisementWatcher;
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
/// How long an Apple headset is listened for: its broadcasts come several times a second.
const SCAN: Duration = Duration::from_secs(8);

/// One Bluetooth output device arriving, leaving, or its battery changing.
pub struct Seen {
    /// Its container id, the same for its every endpoint.
    pub container: String,
    /// Its name is never empty.
    pub identity: Identity,
    pub power: Option<Power>,
    pub change: Change,
}

pub enum Change {
    Connected,
    Disconnected,
    /// Still connected, its batteries no longer what they were.
    Battery {
        was: Option<Power>,
    },
}

enum Heard {
    /// An audio endpoint changed state.
    Endpoint(String, DEVICE_STATE),
    /// A node in this watched container was added, changed or removed.
    Container(String),
    /// An Apple broadcast, at this signal strength.
    Advert(i16, Vec<u8>),
}

/// A connected device: the endpoint it was first heard on, and what its batteries last were.
struct Watched {
    endpoint: String,
    identity: Identity,
    power: Option<Power>,
    watcher: Option<DeviceWatcher>,
    /// Listening for its broadcasts, until then.
    scan: Option<(Scan, Instant)>,
}

impl Watched {
    fn listen(&mut self, now: Instant) {
        if let Some(model) = apple::model_of(&self.identity) {
            self.scan = Some((Scan::new(model), now + SCAN));
        }
    }
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
            let mut scanner: Option<BluetoothLEAdvertisementWatcher> = None;
            loop {
                let until = watched
                    .values()
                    .filter_map(|d| d.scan.as_ref())
                    .map(|s| s.1)
                    .min();
                let wait = until.map_or(Duration::MAX, |at| {
                    at.saturating_duration_since(Instant::now())
                });
                let heard = match rx.recv_timeout(wait) {
                    Ok(heard) => Some(heard),
                    Err(mpsc::RecvTimeoutError::Timeout) => None,
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                };
                let now = Instant::now();
                match heard {
                    None => {}
                    Some(Heard::Endpoint(id, state)) => {
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
                                let mut device = Watched {
                                    endpoint: id,
                                    identity: seen.identity.clone(),
                                    power: seen.power,
                                    watcher: watch_container(&seen.container, &tx).log_err(),
                                    scan: None,
                                };
                                device.listen(now);
                                watched.insert(seen.container.clone(), device);
                            }
                            Change::Connected | Change::Battery { .. } => {}
                        }
                        report(seen);
                    }
                    Some(Heard::Container(container)) => 'read: {
                        let Some(device) = watched.get_mut(&container) else {
                            break 'read;
                        };
                        let was = device.power;
                        // SAFETY: COM calls on the thread that created `devices`.
                        let Some(seen) = (unsafe {
                            look_up(&devices, &device.endpoint, Change::Battery { was })
                        }) else {
                            break 'read;
                        };
                        if seen.power.is_none() || seen.power == was {
                            break 'read;
                        }
                        device.listen(now);
                        // Each bud's battery says more than Windows' one: the buds are asked again.
                        if let Some(Power::Split { .. }) = was {
                            break 'read;
                        }
                        device.power = seen.power;
                        report(seen);
                    }
                    Some(Heard::Advert(rssi, data)) => {
                        for (container, device) in &mut watched {
                            let Some(power) = device
                                .scan
                                .as_mut()
                                .and_then(|(scan, _)| scan.hear(rssi, &data))
                            else {
                                continue;
                            };
                            let was = device.power.replace(power);
                            report(Seen {
                                container: container.clone(),
                                identity: device.identity.clone(),
                                power: Some(power),
                                change: Change::Battery { was },
                            });
                        }
                    }
                }
                for device in watched.values_mut() {
                    device.scan = device.scan.take().filter(|(_, until)| *until > now);
                }
                let scanning = watched.values().any(|d| d.scan.is_some());
                match (scanning, &scanner) {
                    (true, None) => {
                        let wake = tx.clone();
                        scanner = adverts::scan(move |rssi, data| {
                            wake.send(Heard::Advert(rssi, data)).log_err();
                        })
                        .log_err();
                    }
                    (false, Some(watcher)) => {
                        watcher.Stop().log_err();
                        scanner = None;
                    }
                    (true, Some(_)) | (false, None) => {}
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
            power: found.battery.map(Power::Single),
            change,
        })
    }
}
