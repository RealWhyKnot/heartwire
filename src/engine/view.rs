use num_traits::ToPrimitive;
use std::sync::Arc;

use crate::config::Config;
use crate::sources::Device;

pub(crate) enum Event {
    Reading { generation: u64, bpm: u16 },
    Status { generation: u64, text: String },
    Devices { generation: u64, list: Vec<Device> },
    Config(Box<Config>),
    Restart,
    InstallFirmware,
    Quit,
}

impl Event {
    #[cfg(test)]
    pub(crate) fn reading(&self) -> Option<u16> {
        match self {
            Event::Reading { bpm, .. } => Some(*bpm),
            Event::Status { .. }
            | Event::Devices { .. }
            | Event::Config(_)
            | Event::Restart
            | Event::InstallFirmware
            | Event::Quit => None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct View {
    pub connected: bool,
    pub bpm: u16,
    pub percent: f32,
    pub status: String,
    pub devices: Arc<[Device]>,
    pub notice: String,
}

pub(crate) trait Ui {
    fn show(&self, view: &View);
}

pub(crate) fn percent(bpm: u16, max: u32) -> f32 {
    (f32::from(bpm) / max.max(1).to_f32().expect("u32 values fit the f32 range")).clamp(0.0, 1.0)
}
