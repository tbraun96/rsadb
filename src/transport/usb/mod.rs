//! USB transport over [`nusb`]: enumerate ADB interfaces and open them.
//!
//! An ADB interface is any USB interface with class `0xFF`, subclass `0x42`,
//! protocol `0x01`. The device may expose it alongside MTP or other functions.

mod io;

pub use io::{UsbSink, UsbSource, UsbTransport};

use crate::error::{Error, Result};
use nusb::descriptors::TransferType;
use nusb::transfer::{Bulk, Direction, In, Out};
use nusb::{DeviceInfo, ErrorKind};

/// USB interface class for vendor-specific interfaces.
pub const ADB_CLASS: u8 = 0xFF;
/// Subclass Google assigned to ADB.
pub const ADB_SUBCLASS: u8 = 0x42;
/// Protocol number for ADB.
pub const ADB_PROTOCOL: u8 = 0x01;

/// An enumerated ADB-capable USB device.
#[derive(Debug, Clone)]
pub struct UsbDeviceInfo {
    info: DeviceInfo,
    interface_number: u8,
}

impl UsbDeviceInfo {
    /// Wrap a `nusb` record whose ADB interface number is already known (see [`adb_interface`]).
    pub fn from_parts(info: DeviceInfo, interface_number: u8) -> Self {
        Self {
            info,
            interface_number,
        }
    }

    /// The serial number reported by the device, if any.
    pub fn serial(&self) -> Option<&str> {
        self.info.serial_number()
    }

    /// Product string, if any.
    pub fn product(&self) -> Option<&str> {
        self.info.product_string()
    }

    /// Manufacturer string, if any.
    pub fn manufacturer(&self) -> Option<&str> {
        self.info.manufacturer_string()
    }

    /// USB vendor id.
    pub fn vendor_id(&self) -> u16 {
        self.info.vendor_id()
    }

    /// USB product id.
    pub fn product_id(&self) -> u16 {
        self.info.product_id()
    }

    /// The interface number that carries ADB.
    pub fn interface_number(&self) -> u8 {
        self.interface_number
    }

    /// Stable identity for this attachment (changes when re-plugged).
    pub fn id(&self) -> nusb::DeviceId {
        self.info.id()
    }

    /// The underlying `nusb` record.
    pub fn nusb_info(&self) -> &DeviceInfo {
        &self.info
    }
}

/// Whether `info` exposes an ADB interface, and if so which one.
pub fn adb_interface(info: &DeviceInfo) -> Option<u8> {
    info.interfaces()
        .find(|i| {
            i.class() == ADB_CLASS && i.subclass() == ADB_SUBCLASS && i.protocol() == ADB_PROTOCOL
        })
        .map(nusb::InterfaceInfo::interface_number)
}

fn map_usb(e: &nusb::Error) -> Error {
    match e.kind() {
        ErrorKind::Busy | ErrorKind::PermissionDenied => Error::ClaimFailed(e.to_string()),
        ErrorKind::Disconnected => Error::Disconnected,
        _ => Error::Usb(e.to_string()),
    }
}

/// Enumerate every attached device that exposes an ADB interface.
pub async fn list() -> Result<Vec<UsbDeviceInfo>> {
    let devices = nusb::list_devices().await.map_err(|e| map_usb(&e))?;
    Ok(devices
        .filter_map(|info| {
            adb_interface(&info).map(|interface_number| UsbDeviceInfo {
                info,
                interface_number,
            })
        })
        .collect())
}

/// Find a device by serial, or the only device when `serial` is `None`.
pub async fn find(serial: Option<&str>) -> Result<UsbDeviceInfo> {
    let mut devices = list().await?;
    match serial {
        Some(wanted) => devices
            .into_iter()
            .find(|d| d.serial() == Some(wanted))
            .ok_or_else(|| Error::NoDevice(Some(format!("serial {wanted}")))),
        None => match devices.len() {
            0 => Err(Error::NoDevice(None)),
            1 => devices.pop().ok_or(Error::NoDevice(None)),
            n => Err(Error::NoDevice(Some(format!(
                "{n} devices attached, pass a serial"
            )))),
        },
    }
}

/// Open and claim the ADB interface of `device`.
pub async fn open(device: &UsbDeviceInfo) -> Result<UsbTransport> {
    let handle = device.info.open().await.map_err(|e| map_usb(&e))?;
    let interface = handle
        .detach_and_claim_interface(device.interface_number)
        .await
        .map_err(|e| map_usb(&e))?;
    let descriptor = interface
        .descriptor()
        .ok_or_else(|| Error::Usb("interface has no active descriptor".into()))?;
    let mut in_addr = None;
    let mut out_addr = None;
    for ep in descriptor.endpoints() {
        if ep.transfer_type() != TransferType::Bulk {
            continue;
        }
        match ep.direction() {
            Direction::In => in_addr.get_or_insert(ep.address()),
            Direction::Out => out_addr.get_or_insert(ep.address()),
        };
    }
    let (in_addr, out_addr) = in_addr
        .zip(out_addr)
        .ok_or_else(|| Error::Usb("ADB interface lacks bulk IN/OUT endpoints".into()))?;
    let bulk_in = interface
        .endpoint::<Bulk, In>(in_addr)
        .map_err(|e| map_usb(&e))?;
    let bulk_out = interface
        .endpoint::<Bulk, Out>(out_addr)
        .map_err(|e| map_usb(&e))?;
    Ok(UsbTransport::new(handle, interface, bulk_in, bulk_out))
}
