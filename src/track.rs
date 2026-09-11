//! Attach/detach tracking for ADB USB devices.
//!
//! [`watch_usb`] uses the operating system's hotplug notifications through
//! `nusb`; [`poll_usb`] re-enumerates on a timer and works anywhere
//! enumeration does. Both begin by reporting every device already attached.

use crate::error::Result;
use crate::transport::usb::{UsbDeviceInfo, adb_interface, list};
use futures::{Stream, StreamExt as _};
use nusb::DeviceId;
use nusb::hotplug::HotplugEvent;
use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasher, Hash};
use std::time::Duration;

/// A change in the set of attached ADB devices.
#[derive(Debug, Clone)]
pub enum UsbEvent {
    /// A device exposing an ADB interface appeared (boxed: `DeviceInfo` is large on Windows).
    Attached(Box<UsbDeviceInfo>),
    /// A previously reported device went away.
    Detached(DeviceId),
}

/// Compare two snapshots keyed by identity: `(added, removed)` in key order of iteration.
pub fn diff<'a, K: Hash + Eq, V, S: BuildHasher>(
    previous: &'a HashMap<K, V, S>,
    current: &'a HashMap<K, V, S>,
) -> (Vec<&'a V>, Vec<&'a K>) {
    let added = current
        .iter()
        .filter(|(k, _)| !previous.contains_key(k))
        .map(|(_, v)| v)
        .collect();
    let removed = previous
        .keys()
        .filter(|k| !current.contains_key(k))
        .collect();
    (added, removed)
}

/// Hotplug-driven stream of events (Linux, macOS, Windows).
pub fn watch_usb() -> Result<impl Stream<Item = Result<UsbEvent>> + Send> {
    let hotplug = nusb::watch_devices().map_err(|e| crate::Error::Usb(e.to_string()))?;
    let initial = futures::stream::once(async {
        list().await.map(|devices| {
            let ids: HashSet<_> = devices.iter().map(UsbDeviceInfo::id).collect();
            (
                devices
                    .into_iter()
                    .map(|d| Ok(UsbEvent::Attached(Box::new(d))))
                    .collect::<Vec<_>>(),
                ids,
            )
        })
    });
    let mut hotplug = Some(hotplug);
    Ok(
        initial.flat_map(move |snapshot| match (snapshot, hotplug.take()) {
            (Ok((events, known)), Some(watch)) => futures::stream::iter(events)
                .chain(hotplug_events(watch, known))
                .left_stream(),
            (Err(e), _) => futures::stream::iter(vec![Err(e)]).right_stream(),
            (Ok(_), None) => futures::stream::iter(Vec::new()).right_stream(),
        }),
    )
}

fn hotplug_events(
    watch: nusb::hotplug::HotplugWatch,
    known: HashSet<DeviceId>,
) -> impl Stream<Item = Result<UsbEvent>> + Send {
    futures::stream::unfold((watch, known), |(mut watch, mut known)| async move {
        loop {
            let event = watch.next().await?;
            match event {
                HotplugEvent::Connected(info) => {
                    if let Some(interface_number) = adb_interface(&info) {
                        known.insert(info.id());
                        let device = UsbDeviceInfo::from_parts(info, interface_number);
                        return Some((Ok(UsbEvent::Attached(Box::new(device))), (watch, known)));
                    }
                }
                HotplugEvent::Disconnected(id) => {
                    if known.remove(&id) {
                        return Some((Ok(UsbEvent::Detached(id)), (watch, known)));
                    }
                }
            }
        }
    })
}

/// Polling stream of events: re-enumerates every `interval`.
pub fn poll_usb(interval: Duration) -> impl Stream<Item = Result<UsbEvent>> + Send {
    futures::stream::unfold(
        (HashMap::<DeviceId, UsbDeviceInfo>::new(), true),
        move |(known, first)| async move {
            if !first {
                tokio::time::sleep(interval).await;
            }
            let current: HashMap<_, _> = match list().await {
                Ok(devices) => devices.into_iter().map(|d| (d.id(), d)).collect(),
                Err(e) => return Some((vec![Err(e)], (known, false))),
            };
            let (added, removed) = diff(&known, &current);
            let mut events: Vec<Result<UsbEvent>> = removed
                .into_iter()
                .map(|id| Ok(UsbEvent::Detached(*id)))
                .collect();
            events.extend(
                added
                    .into_iter()
                    .map(|d| Ok(UsbEvent::Attached(Box::new(d.clone())))),
            );
            Some((events, (current, false)))
        },
    )
    .flat_map(futures::stream::iter)
}

#[cfg(test)]
mod tests {
    use super::diff;
    use std::collections::HashMap;

    #[test]
    fn diff_reports_added_and_removed() {
        let prev: HashMap<&str, u8> = [("a", 1), ("b", 2)].into();
        let next: HashMap<&str, u8> = [("b", 2), ("c", 3)].into();
        let (added, removed) = diff(&prev, &next);
        assert_eq!(added, vec![&3]);
        assert_eq!(removed, vec![&"a"]);
        let (none_added, none_removed) = diff(&next, &next);
        assert!(none_added.is_empty() && none_removed.is_empty());
    }
}
