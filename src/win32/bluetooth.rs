//! What Configuration Manager knows about Bluetooth devices: a device's nodes share a container
//! id, and between them carry its name, its Bluetooth class, its product ids and, where Windows
//! knows it, its battery.

use std::collections::BTreeMap;

use windows::Win32::Devices::DeviceAndDriverInstallation::{
    CM_GETIDLIST_FILTER_ENUMERATOR, CM_Get_DevNode_PropertyW, CM_Get_Device_ID_List_SizeW,
    CM_Get_Device_ID_ListW, CM_LOCATE_DEVNODE_NORMAL, CM_LOCATE_DEVNODE_PHANTOM,
    CM_Locate_DevNodeW, CR_SUCCESS,
};
use windows::Win32::Devices::Properties::DEVPROPTYPE;
use windows::Win32::Foundation::DEVPROPKEY;
use windows::core::{GUID, HSTRING, PCWSTR};

use model::headset::{Dumped, Identity, Product, redact_address};

pub const CONTAINER: GUID = GUID::from_u128(0x8c7ed206_3f8a_4827_b3ab_ae9e1faefc6c);
const FRIENDLY: DEVPROPKEY = DEVPROPKEY {
    fmtid: GUID::from_u128(0xa45c254e_df1c_4efd_8020_67d146a850e0),
    pid: 14,
};
/// Bluetooth's battery level, a byte from 0 to 100, on whichever of the device's nodes knows it.
const BATTERY: DEVPROPKEY = DEVPROPKEY {
    fmtid: GUID::from_u128(0x104ea319_6ee2_4701_bd47_8ddbf425bbe5),
    pid: 2,
};
/// `BATTERY` as `DeviceInformation` names a property.
pub const BATTERY_NAME: &str = "{104EA319-6EE2-4701-BD47-8DDBF425BBE5} 2";
/// `DEVPKEY_Bluetooth_ClassOfDevice`, on a classic device's own node.
const CLASS: DEVPROPKEY = DEVPROPKEY {
    fmtid: GUID::from_u128(0x2bd67d8b_8beb_48d5_87e0_6cda3428040a),
    pid: 10,
};
/// `DEVPKEY_Bluetooth_DeviceVIDSource`, `_DeviceVID` and `_DevicePID`: the Device ID profile's
/// fields, on the device's own node, for devices whose profile ids don't carry them.
const VID_SOURCE: DEVPROPKEY = DEVPROPKEY {
    fmtid: GUID::from_u128(0x2bd67d8b_8beb_48d5_87e0_6cda3428040a),
    pid: 6,
};
const VID: DEVPROPKEY = DEVPROPKEY {
    fmtid: GUID::from_u128(0x2bd67d8b_8beb_48d5_87e0_6cda3428040a),
    pid: 7,
};
const PID: DEVPROPKEY = DEVPROPKEY {
    fmtid: GUID::from_u128(0x2bd67d8b_8beb_48d5_87e0_6cda3428040a),
    pid: 8,
};
const ENUMERATORS: [&str; 4] = ["BTHENUM", "BTHLE", "BTHHFENUM", "BTHLEDEVICE"];

/// A Bluetooth device, from its nodes.
pub struct Found {
    /// Its name is empty when no node carries one.
    pub identity: Identity,
    pub battery: Option<u8>,
    /// Its nodes' instance ids, addresses blanked.
    pub ids: Vec<String>,
    /// Its own node is there now, not only remembered.
    pub present: bool,
}

/// The Bluetooth device in `container`, if there is one.
pub fn in_container(container: GUID) -> Option<Found> {
    let nodes: Vec<_> = nodes().filter(|node| node.container == container).collect();
    (!nodes.is_empty()).then(|| found(&nodes))
}

/// Every paired Bluetooth device.
pub fn dump() -> Vec<Dumped> {
    let mut by_container: BTreeMap<u128, Vec<Node>> = BTreeMap::new();
    for node in nodes() {
        by_container
            .entry(node.container.to_u128())
            .or_default()
            .push(node);
    }
    by_container
        .values()
        .filter(|nodes| nodes.iter().any(Node::is_device))
        .map(|nodes| {
            let found = found(nodes);
            Dumped {
                identity: found.identity,
                present: found.present,
                battery: found.battery,
                ids: found.ids,
            }
        })
        .collect()
}

struct Node {
    id: String,
    node: u32,
    container: GUID,
}

impl Node {
    /// The device's own node (`BTHENUM\DEV_...`, `BTHLE\DEV_...`), not one of its profiles.
    fn is_device(&self) -> bool {
        self.id.to_ascii_uppercase().contains("\\DEV_")
    }
}

fn found(nodes: &[Node]) -> Found {
    let device = || nodes.iter().filter(|node| node.is_device());
    // SAFETY: each node was located just now; the properties are read into owned buffers.
    unsafe {
        Found {
            identity: Identity {
                name: device()
                    .find_map(|node| text(node.node, &FRIENDLY))
                    .unwrap_or_default(),
                product: nodes
                    .iter()
                    .find_map(|node| Product::from_device_id(&node.id))
                    .or_else(|| device().find_map(|node| product_of(node.node))),
                class: device()
                    .find_map(|node| property::<4>(node.node, &CLASS))
                    .map(u32::from_le_bytes),
            },
            battery: nodes
                .iter()
                .find_map(|node| property::<1>(node.node, &BATTERY))
                .map(|b| b[0].min(100)),
            ids: nodes.iter().map(|node| redact_address(&node.id)).collect(),
            present: device().any(|node| {
                let mut present = 0;
                let id = HSTRING::from(node.id.as_str());
                CM_Locate_DevNodeW(&mut present, &id, CM_LOCATE_DEVNODE_NORMAL) == CR_SUCCESS
            }),
        }
    }
}

/// The Device ID profile's fields on a device's own node.
unsafe fn product_of(node: u32) -> Option<Product> {
    unsafe {
        let source = property::<1>(node, &VID_SOURCE)?[0];
        let vid = u16::from_le_bytes(property::<2>(node, &VID)?);
        let pid = u16::from_le_bytes(property::<2>(node, &PID)?);
        Product::from_parts(source.into(), vid, pid)
    }
}

/// Every Bluetooth node, present or remembered, with its container.
fn nodes() -> impl Iterator<Item = Node> {
    ENUMERATORS
        .into_iter()
        // SAFETY: the list is read into an owned buffer.
        .flat_map(|enumerator| unsafe { device_ids(enumerator) })
        .filter_map(|id| {
            let mut node = 0;
            // SAFETY: `id` is NUL-terminated.
            let located = unsafe {
                CM_Locate_DevNodeW(&mut node, PCWSTR(id.as_ptr()), CM_LOCATE_DEVNODE_PHANTOM)
            };
            if located != CR_SUCCESS {
                return None;
            }
            let key = DEVPROPKEY {
                fmtid: CONTAINER,
                pid: 2,
            };
            // SAFETY: a GUID is 16 plain bytes, stored as Windows lays it out.
            let container = unsafe { property::<16>(node, &key) }
                .map(|bytes| unsafe { std::ptr::read_unaligned(bytes.as_ptr().cast::<GUID>()) })?;
            let id = String::from_utf16_lossy(&id[..id.len() - 1]);
            Some(Node {
                id,
                node,
                container,
            })
        })
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
