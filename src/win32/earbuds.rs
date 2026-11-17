//! Bluetooth audio coming and going. An `IMMNotificationClient` hears an output endpoint turn
//! active or go away and hands its id to a thread of its own, which asks Configuration Manager
//! whether the endpoint's device container is a Bluetooth one, for its name, and for its
//! battery where Windows knows it. Nothing polls.

use std::sync::mpsc;

use windows::Win32::Devices::DeviceAndDriverInstallation::{
    CM_GETIDLIST_FILTER_ENUMERATOR, CM_Get_DevNode_PropertyW, CM_Get_Device_ID_List_SizeW,
    CM_Get_Device_ID_ListW, CM_LOCATE_DEVNODE_PHANTOM, CM_Locate_DevNodeW, CR_SUCCESS,
};
use windows::Win32::Devices::Properties::DEVPROPTYPE;
use windows::Win32::Foundation::{DEVPROPKEY, PROPERTYKEY};
use windows::Win32::Media::Audio::{
    DEVICE_STATE, DEVICE_STATE_ACTIVE, DEVICE_STATE_NOTPRESENT, DEVICE_STATE_UNPLUGGED, EDataFlow,
    ERole, IMMDeviceEnumerator, IMMEndpoint, IMMNotificationClient, IMMNotificationClient_Impl,
    MMDeviceEnumerator, eRender,
};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, STGM_READ,
};
use windows::Win32::System::Variant::VT_CLSID;
use windows::core::{GUID, HSTRING, Interface, PCWSTR, implement};

const CONTAINER: GUID = GUID::from_u128(0x8c7ed206_3f8a_4827_b3ab_ae9e1faefc6c);
const FRIENDLY: GUID = GUID::from_u128(0xa45c254e_df1c_4efd_8020_67d146a850e0);
const INTERFACE_NAME: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0x026e516e_b814_414b_83cd_856d6fef4822),
    pid: 2,
};
/// Bluetooth's battery level, a byte from 0 to 100, on whichever of the device's nodes knows it.
const BATTERY: DEVPROPKEY = DEVPROPKEY {
    fmtid: GUID::from_u128(0x104ea319_6ee2_4701_bd47_8ddbf425bbe5),
    pid: 2,
};
const ENUMERATORS: [&str; 4] = ["BTHENUM", "BTHLE", "BTHHFENUM", "BTHLEDEVICE"];

/// One Bluetooth output device arriving or leaving.
pub struct Seen {
    /// Its container id, the same for its every endpoint.
    pub container: String,
    pub name: String,
    pub battery: Option<u8>,
    pub connected: bool,
}

#[implement(IMMNotificationClient)]
struct Client(mpsc::Sender<(String, DEVICE_STATE)>);

impl IMMNotificationClient_Impl for Client_Impl {
    fn OnDeviceStateChanged(&self, id: &PCWSTR, state: DEVICE_STATE) -> windows::core::Result<()> {
        // SAFETY: the id is valid for this call.
        if let Ok(id) = unsafe { id.to_string() } {
            let _ = self.0.send((id, state));
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
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL)
                    .and_then(|devices| {
                        let client: IMMNotificationClient = Client(tx).into();
                        devices.RegisterEndpointNotificationCallback(&client)?;
                        Ok((devices, client))
                    })
            };
            let (devices, _client) = match devices {
                Ok(devices) => devices,
                Err(err) => return eprintln!("topsy: no audio device events: {err}"),
            };
            for (id, state) in rx {
                let connected = match state {
                    DEVICE_STATE_ACTIVE => true,
                    DEVICE_STATE_NOTPRESENT | DEVICE_STATE_UNPLUGGED => false,
                    _ => continue,
                };
                // SAFETY: COM calls on the thread that created `devices`.
                if let Some(seen) = unsafe { look_up(&devices, &id, connected) } {
                    report(seen);
                }
            }
        });
    if let Err(err) = spawned {
        eprintln!("topsy: no audio device events: {err}");
    }
}

/// The endpoint `id` as a Bluetooth output, or `None` if it isn't one.
unsafe fn look_up(devices: &IMMDeviceEnumerator, id: &str, connected: bool) -> Option<Seen> {
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
        let (name, battery) = bluetooth(container)?;
        Some(Seen {
            container: format!("{container:?}"),
            name: name.or(endpoint_name)?,
            battery,
            connected,
        })
    }
}

/// The Bluetooth device in `container`, if there is one: its name and its battery.
unsafe fn bluetooth(container: GUID) -> Option<(Option<String>, Option<u8>)> {
    let mut found = false;
    let (mut name, mut battery) = (None, None);
    for enumerator in ENUMERATORS {
        for id in unsafe { device_ids(enumerator) } {
            let mut node = 0;
            // SAFETY: `id` is NUL-terminated.
            let located = unsafe {
                CM_Locate_DevNodeW(&mut node, PCWSTR(id.as_ptr()), CM_LOCATE_DEVNODE_PHANTOM)
            };
            let container_key = DEVPROPKEY {
                fmtid: CONTAINER,
                pid: 2,
            };
            if located != CR_SUCCESS
                || unsafe { property::<16>(node, &container_key) }
                    // SAFETY: a GUID is 16 plain bytes, stored as Windows lays it out.
                    .map(|bytes| unsafe { std::ptr::read_unaligned(bytes.as_ptr().cast::<GUID>()) })
                    != Some(container)
            {
                continue;
            }
            found = true;
            battery =
                battery.or_else(|| unsafe { property::<1>(node, &BATTERY) }.map(|b| b[0].min(100)));
            // The device's own node (`BTHENUM\DEV_...`) carries the name the user knows it by.
            let is_device = String::from_utf16_lossy(&id)
                .to_ascii_uppercase()
                .contains("\\DEV_");
            if name.is_none() && is_device {
                name = unsafe {
                    text(
                        node,
                        &DEVPROPKEY {
                            fmtid: FRIENDLY,
                            pid: 14,
                        },
                    )
                };
            }
        }
    }
    found.then_some((name, battery))
}

/// Every device instance id under `enumerator`, present or not, each ending in its NUL.
unsafe fn device_ids(enumerator: &str) -> Vec<Vec<u16>> {
    let filter = HSTRING::from(enumerator);
    let mut len = 0u32;
    unsafe {
        if CM_Get_Device_ID_List_SizeW(&mut len, &filter, CM_GETIDLIST_FILTER_ENUMERATOR)
            != CR_SUCCESS
        {
            return Vec::new();
        }
        let mut list = vec![0u16; len as usize];
        if CM_Get_Device_ID_ListW(&filter, &mut list, CM_GETIDLIST_FILTER_ENUMERATOR) != CR_SUCCESS
        {
            return Vec::new();
        }
        list.split(|&c| c == 0)
            .filter(|id| !id.is_empty())
            .map(|id| id.iter().copied().chain([0]).collect())
            .collect()
    }
}

/// A fixed-size property of a device node.
unsafe fn property<const N: usize>(node: u32, key: &DEVPROPKEY) -> Option<[u8; N]> {
    let mut kind = DEVPROPTYPE::default();
    let mut buffer = [0u8; N];
    let mut size = N as u32;
    let got = unsafe {
        CM_Get_DevNode_PropertyW(
            node,
            key,
            &mut kind,
            Some(buffer.as_mut_ptr()),
            &mut size,
            0,
        )
    };
    (got == CR_SUCCESS && size as usize == N).then_some(buffer)
}

/// A string property of a device node.
unsafe fn text(node: u32, key: &DEVPROPKEY) -> Option<String> {
    let mut kind = DEVPROPTYPE::default();
    let mut buffer = [0u16; 256];
    let mut size = (buffer.len() * 2) as u32;
    let got = unsafe {
        CM_Get_DevNode_PropertyW(
            node,
            key,
            &mut kind,
            Some(buffer.as_mut_ptr().cast()),
            &mut size,
            0,
        )
    };
    if got != CR_SUCCESS {
        return None;
    }
    let text = &buffer[..(size as usize / 2).min(buffer.len())];
    let text = String::from_utf16_lossy(text.split(|&c| c == 0).next()?);
    (!text.is_empty()).then_some(text)
}
