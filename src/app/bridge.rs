use std::sync::mpsc::Sender;

use slint::ComponentHandle;

use super::state::App;
use super::{devices, taskbar};
use crate::engine::{self, View};
use crate::steamvr;
use crate::ui::{AppWindow, HeartRate};

pub struct Bridge {
    window: slint::Weak<AppWindow>,
    vr: Sender<steamvr::Msg>,
}

impl Bridge {
    pub fn new(window: &AppWindow, vr: Sender<steamvr::Msg>) -> Bridge {
        Bridge {
            window: window.as_weak(),
            vr,
        }
    }
}

impl engine::Ui for Bridge {
    fn show(&self, view: &View) {
        let percent = format!("{:.2}", view.percent);
        let _ = self.vr.send(steamvr::Msg::View(steamvr::PanelView {
            connected: view.connected,
            bpm: view.bpm,
            percent: percent.clone(),
            status: view.status.clone(),
        }));
        let view = view.clone();
        let _ = self.window.upgrade_in_event_loop(move |window| {
            taskbar::update(view.connected, view.bpm);
            let heart_rate = window.global::<HeartRate>();
            heart_rate.set_connected(view.connected);
            heart_rate.set_bpm(i32::from(view.bpm));
            heart_rate.set_percent_text(percent.into());
            heart_rate.set_status(view.status.as_str().into());
            heart_rate.set_notice(view.notice.as_str().into());
            App::with(|app| {
                if app.set_devices(view.devices) {
                    heart_rate.set_devices(devices::model(&app.devices(), &app.pinned_device()));
                }
            });
        });
    }
}
