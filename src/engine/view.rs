use std::sync::Arc;

use crate::config::Config;
use crate::sources::Device;

pub enum Event {
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
    pub fn reading(&self) -> Option<u16> {
        match self {
            Event::Reading { bpm, .. } => Some(*bpm),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct View {
    pub connected: bool,
    pub bpm: u16,
    pub percent: f32,
    pub status: String,
    pub devices: Arc<[Device]>,
    pub notice: String,
}

pub trait Ui {
    fn show(&self, view: &View);
}

pub fn percent(bpm: u16, max: u32) -> f32 {
    (f32::from(bpm) / max.max(1) as f32).clamp(0.0, 1.0)
}
