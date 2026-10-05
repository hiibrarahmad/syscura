//! USB devices plugged into physical ports, with port/hub numbers and the
//! panel (front/back) when the firmware describes it.

use serde::Deserialize;
use syscura_core::hw::UsbDevice;
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    CM_Get_Child, CM_Get_DevNode_PropertyW, CM_Get_Parent, CM_Get_Sibling, CM_LOCATE_DEVNODE_NORMAL,
    CM_Locate_DevNodeW, CR_SUCCESS,
};
use windows::Win32::Devices::Properties::{
    DEVPKEY_Device_BusReportedDeviceDesc, DEVPKEY_Device_Class, DEVPKEY_Device_DeviceDesc,
    DEVPKEY_Device_FriendlyName, DEVPKEY_Device_LocationInfo, DEVPKEY_Device_LocationPaths,
    DEVPKEY_Device_PhysicalDeviceLocation, DEVPROPTYPE,
};
use windows::Win32::Foundation::DEVPROPKEY;
use windows::core::HSTRING;
use wmi::WMIConnection;

use super::{query, text};
use crate::parse;

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct PnpEntity {
    #[serde(rename = "PNPDeviceID")]
    pnp_device_id: Option<String>,
    #[serde(rename = "PNPClass")]
    pnp_class: Option<String>,
    manufacturer: Option<String>,
}

/// Names Windows gives to devices it has no better name for.
const GENERIC: &[&str] = &[
    "USB Composite Device",
    "USB Input Device",
    "USB Mass Storage Device",
    "USB Audio Device",
    "USB Video Device",
    "HID-compliant device",
];

pub fn devices(con: &WMIConnection) -> Vec<UsbDevice> {
    let entities: Vec<PnpEntity> = query(
        con,
        "SELECT PNPDeviceID, PNPClass, Manufacturer FROM Win32_PnPEntity \
         WHERE PNPDeviceID LIKE 'USB\\\\%' AND Present = TRUE",
    );
    let mut out = Vec::new();
    for e in entities {
        let id = text(e.pnp_device_id);
        let Some(node) = locate(&id) else { continue };
        let location = prop_string(node, &DEVPKEY_Device_LocationInfo).unwrap_or_default();
        // Only nodes attached directly to a port; interface children and
        // root hubs have no "Port_#" location.
        let (port, hub) = parse::usb_location(&location);
        if port.is_none() {
            continue;
        }
        let name = best_name(node);
        let class = text(e.pnp_class);
        let kind = match parse::usb_kind(&class, &name) {
            "Other" => descendant_kind(node).unwrap_or("Other"),
            k => k,
        };
        if kind == "Hub" && name.to_ascii_lowercase().contains("root") {
            continue;
        }
        let panel = prop_bytes(node, &DEVPKEY_Device_PhysicalDeviceLocation)
            .map(|pld| parse::pld_panel(&pld))
            // Many boards leave the panel field at its default; only an
            // explicit front or back is worth showing.
            .map(parse::desktop_panel)
            .unwrap_or_default();
        out.push(UsbDevice {
            name,
            kind: kind.into(),
            manufacturer: text(e.manufacturer),
            instance_id: id,
            location,
            port,
            hub,
            panel,
        });
    }
    out.sort_by_key(|d| (d.hub, d.port));
    out
}

/// The product name the device reports about itself beats Windows' generic
/// driver names ("USB Composite Device").
fn best_name(node: u32) -> String {
    let friendly = prop_string(node, &DEVPKEY_Device_FriendlyName)
        .or_else(|| prop_string(node, &DEVPKEY_Device_DeviceDesc))
        .unwrap_or_default();
    if !GENERIC.iter().any(|g| friendly.eq_ignore_ascii_case(g)) && !friendly.is_empty() {
        return friendly;
    }
    prop_string(node, &DEVPKEY_Device_BusReportedDeviceDesc)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(friendly)
}

/// Looks at child devices (up to three levels down) to tell what a
/// composite device really is.
fn descendant_kind(node: u32) -> Option<&'static str> {
    let mut classes = Vec::new();
    collect_classes(node, 0, &mut classes);
    let has = |c: &str| classes.iter().any(|x| x.eq_ignore_ascii_case(c));
    [
        ("DiskDrive", "Storage"),
        ("Camera", "Camera"),
        ("Image", "Camera"),
        ("AudioEndpoint", "Audio"),
        ("Media", "Audio"),
        ("Bluetooth", "Bluetooth"),
        ("Net", "Network"),
    ]
    .into_iter()
    .find(|(class, _)| has(class))
    .map(|(_, kind)| kind)
    // Gaming mice expose a keyboard interface for macro keys and gaming
    // keyboards expose a mouse one; the first interface is the real job.
    .or_else(|| {
        classes.iter().find_map(|c| match c.as_str() {
            "Mouse" => Some("Mouse"),
            "Keyboard" => Some("Keyboard"),
            _ => None,
        })
    })
}

fn collect_classes(node: u32, depth: u32, out: &mut Vec<String>) {
    if depth > 3 {
        return;
    }
    let mut child = 0u32;
    if unsafe { CM_Get_Child(&mut child, node, 0) } != CR_SUCCESS {
        return;
    }
    loop {
        if let Some(c) = prop_string(child, &DEVPKEY_Device_Class) {
            out.push(c);
        }
        collect_classes(child, depth + 1, out);
        let mut next = 0u32;
        if unsafe { CM_Get_Sibling(&mut next, child, 0) } != CR_SUCCESS {
            break;
        }
        child = next;
    }
}

/// First PCI location path of a device, e.g. "PCIROOT(0)#PCI(0301)#PCI(0000)".
pub fn location_path(instance_id: &str) -> Option<String> {
    node_location_path(locate(instance_id)?)
}

/// Location path of the device's parent (a disk's controller).
pub fn parent_location_path(instance_id: &str) -> Option<String> {
    let node = locate(instance_id)?;
    let mut parent = 0u32;
    if unsafe { CM_Get_Parent(&mut parent, node, 0) } != CR_SUCCESS {
        return None;
    }
    node_location_path(parent)
}

fn node_location_path(node: u32) -> Option<String> {
    // A multi-string; the first entry is the PCIROOT path.
    let bytes = prop_bytes(node, &DEVPKEY_Device_LocationPaths)?;
    let wide: Vec<u16> = bytes.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
    let end = wide.iter().position(|&c| c == 0).unwrap_or(wide.len());
    let first = String::from_utf16_lossy(&wide[..end]);
    (!first.is_empty()).then_some(first)
}

fn locate(instance_id: &str) -> Option<u32> {
    let mut node = 0u32;
    let r = unsafe { CM_Locate_DevNodeW(&mut node, &HSTRING::from(instance_id), CM_LOCATE_DEVNODE_NORMAL) };
    (r == CR_SUCCESS).then_some(node)
}

fn prop_bytes(node: u32, key: &DEVPROPKEY) -> Option<Vec<u8>> {
    let mut ty = DEVPROPTYPE::default();
    let mut size = 0u32;
    // First call reports the size.
    let _ = unsafe { CM_Get_DevNode_PropertyW(node, key, &mut ty, None, &mut size, 0) };
    if size == 0 || size > 64 * 1024 {
        return None;
    }
    let mut buf = vec![0u8; size as usize];
    let r = unsafe { CM_Get_DevNode_PropertyW(node, key, &mut ty, Some(buf.as_mut_ptr()), &mut size, 0) };
    (r == CR_SUCCESS).then(|| {
        buf.truncate(size as usize);
        buf
    })
}

fn prop_string(node: u32, key: &DEVPROPKEY) -> Option<String> {
    let bytes = prop_bytes(node, key)?;
    let wide: Vec<u16> = bytes.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
    let len = wide.iter().position(|&c| c == 0).unwrap_or(wide.len());
    let s = String::from_utf16_lossy(&wide[..len]).trim().to_string();
    (!s.is_empty()).then_some(s)
}
