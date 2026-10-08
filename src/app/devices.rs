use slint::{ModelRc, SharedString, VecModel};

use crate::sources::Device;
use crate::ui::DeviceRow;

pub fn rows(devices: &[Device], pinned: &str) -> Vec<DeviceRow> {
    let pinned = pinned.trim();
    let mut rows = vec![DeviceRow {
        name: "Any heart rate device".into(),
        address: SharedString::new(),
        selected: pinned.is_empty(),
    }];
    let mut found = false;
    for device in devices {
        let selected = !pinned.is_empty() && device.address.eq_ignore_ascii_case(pinned);
        found |= selected;
        let name = if device.name.is_empty() {
            device.address.clone()
        } else {
            format!("{} ({})", device.name, device.address)
        };
        rows.push(DeviceRow {
            name: name.into(),
            address: device.address.as_str().into(),
            selected,
        });
    }
    if !pinned.is_empty() && !found {
        rows.push(DeviceRow {
            name: format!("{pinned} (not seen)").into(),
            address: pinned.into(),
            selected: true,
        });
    }
    rows
}

pub fn model(devices: &[Device], pinned: &str) -> ModelRc<DeviceRow> {
    ModelRc::new(VecModel::from(rows(devices, pinned)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(name: &str, address: &str) -> Device {
        Device {
            name: name.into(),
            address: address.into(),
        }
    }

    #[test]
    fn any_device_comes_first_and_is_picked_by_default() {
        let list = rows(&[device("Polar H10", "C1:22:33:44:55:66")], "");
        assert_eq!(list[0].name, "Any heart rate device");
        assert!(list[0].selected);
        assert_eq!(list[1].name, "Polar H10 (C1:22:33:44:55:66)");
        assert!(!list[1].selected);
    }

    #[test]
    fn a_pinned_device_is_selected_whatever_the_case() {
        let list = rows(&[device("", "C0:FF:EE:00:11:22")], "c0:ff:ee:00:11:22");
        assert!(!list[0].selected);
        assert_eq!(list[1].name, "C0:FF:EE:00:11:22");
        assert!(list[1].selected);
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn a_pinned_device_out_of_range_still_shows() {
        let list = rows(&[], "C0:FF:EE:00:11:22");
        assert_eq!(list.len(), 2);
        assert_eq!(list[1].name, "C0:FF:EE:00:11:22 (not seen)");
        assert!(list[1].selected);
    }
}
