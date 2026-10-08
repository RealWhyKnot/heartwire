use std::sync::Arc;
use std::sync::mpsc::Sender;

use slint::ComponentHandle;

use super::settings::Shared;
use super::{devices, taskbar};
use crate::engine::{self, View};
use crate::steamvr;
use crate::ui::{AppWindow, HeartRate};

pub struct Bridge {
    window: slint::Weak<AppWindow>,
    shared: Arc<Shared>,
    vr: Sender<steamvr::Msg>,
}

impl Bridge {
    pub fn new(window: &AppWindow, shared: Arc<Shared>, vr: Sender<steamvr::Msg>) -> Bridge {
        Bridge {
            window: window.as_weak(),
            shared,
            vr,
        }
    }
}

impl engine::Ui for Bridge {
    fn show(&self, view: &View) {
        let percent = format!("{:.2}", view.percent);
        let _ = self.vr.send(steamvr::Msg::View {
            connected: view.connected,
            bpm: view.bpm,
            percent: percent.clone(),
            status: view.status.clone(),
        });
        let view = view.clone();
        let shared = self.shared.clone();
        let _ = self.window.upgrade_in_event_loop(move |window| {
            taskbar::update(view.connected, view.bpm);
            let heart_rate = window.global::<HeartRate>();
            heart_rate.set_connected(view.connected);
            heart_rate.set_bpm(i32::from(view.bpm));
            heart_rate.set_percent_text(percent.into());
            heart_rate.set_status(view.status.as_str().into());
            heart_rate.set_notice(view.notice.as_str().into());
            let mut found = shared.devices.lock().unwrap();
            if *found != view.devices {
                *found = view.devices;
                heart_rate.set_devices(devices::model(&found, &shared.pinned_device()));
            }
        });
    }
}
