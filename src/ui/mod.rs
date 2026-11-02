#[cfg(any(windows, test))]
pub(crate) use heartwire_ui::VrPanel;
pub(crate) use heartwire_ui::{
    AppWindow, DeviceRow, HeartRate, Navigation, Settings, Updates, Welcome,
};

#[cfg(test)]
mod tests;
