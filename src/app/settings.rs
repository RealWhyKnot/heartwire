use std::rc::Rc;
use std::time::Duration;

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use super::devices;
use super::state::App;
use crate::config::{Config, Service};
use crate::engine::{self, Event};
use crate::ui::{AppWindow, HeartRate, Navigation, Settings, Updates};
use crate::{platform, steamvr, version};

const TYPING_PAUSE: Duration = Duration::from_millis(500);

pub(super) fn load(window: &AppWindow, config: &Config) {
    let settings = window.global::<Settings>();
    let services: Vec<SharedString> = Service::ALL.iter().map(|s| s.label().into()).collect();
    settings.set_services(ModelRc::new(VecModel::from(services)));
    settings.set_service(
        i32::try_from(config.service_type.index()).expect("the service list has five entries"),
    );
    settings.set_connected_timeout(config.connected_timeout.to_string().into());
    settings.set_http_port(config.http_server_port.to_string().into());
    settings.set_widget_id(config.stromno_widget_id.as_str().into());
    settings.set_osc_host(config.osc_client_host.as_str().into());
    settings.set_osc_port(config.osc_client_port.to_string().into());
    settings.set_path_connected(config.osc_path_connected.as_str().into());
    settings.set_path_percent(config.osc_path_percent.as_str().into());
    settings.set_max_heart_rate(config.max_heart_rate.to_string().into());
    settings.set_pico_strap(config.pico_strap_name.as_str().into());
    settings.set_check_updates(config.check_updates);
    settings.set_steamvr(config.steamvr_autostart);
    settings.set_steamvr_supported(steamvr::SUPPORTED);
    settings.set_autostart(platform::autostart_enabled());
    window
        .global::<Updates>()
        .set_current_version(version::VERSION.into());
    let heart_rate = window.global::<HeartRate>();
    heart_rate.set_percent_text(format!("{:.2}", engine::percent(0, config.max_heart_rate)).into());
    heart_rate.set_devices(devices::model(&[], &config.bluetooth_device));
}

pub(super) fn bind(window: &AppWindow, app: &Rc<App>, vr: steamvr::Sender) {
    let settings = window.global::<Settings>();
    {
        let app = std::rc::Rc::clone(app);
        settings.on_setting(move |key, value| {
            app.edit(TYPING_PAUSE, |c| c.set(&key, &value));
        });
    }
    {
        let app = std::rc::Rc::clone(app);
        let weak = window.as_weak();
        settings.on_service_picked(move |index| {
            let service = usize::try_from(index)
                .ok()
                .and_then(|index| Service::ALL.get(index))
                .copied()
                .unwrap_or(Service::Auto);
            app.edit(Duration::ZERO, |c| {
                let changed = c.service_type != service;
                c.service_type = service;
                changed
            });
            if let Some(w) = weak.upgrade() {
                w.global::<Navigation>().set_tab("general".into());
            }
        });
    }
    {
        let app = std::rc::Rc::clone(app);
        let weak = window.as_weak();
        settings.on_device_picked(move |address| {
            app.edit(Duration::ZERO, |c| {
                let changed = c.bluetooth_device != address.as_str();
                c.bluetooth_device = address.to_string();
                changed
            });
            if let Some(w) = weak.upgrade() {
                w.global::<HeartRate>()
                    .set_devices(devices::model(&app.devices(), &app.pinned_device()));
            }
        });
    }
    {
        let weak = window.as_weak();
        settings.on_autostart_toggled(move |on| {
            if let Err(error) = platform::set_autostart(on) {
                tracing::error!(error = %error, "autostart: {error}");
            }
            if let Some(w) = weak.upgrade() {
                w.global::<Settings>()
                    .set_autostart(platform::autostart_enabled());
            }
        });
    }
    {
        let app = std::rc::Rc::clone(app);
        settings.on_steamvr_toggled(move |on| {
            app.edit(Duration::ZERO, |c| {
                c.steamvr_autostart = on;
                true
            });
            vr.enable(on);
        });
    }
    {
        let app = std::rc::Rc::clone(app);
        settings.on_updates_toggled(move |on| {
            app.edit(Duration::ZERO, |c| {
                c.check_updates = on;
                true
            });
        });
    }
    {
        let app = std::rc::Rc::clone(app);
        settings.on_restart_source(move || {
            app.commit(Duration::ZERO);
            app.send(Event::Restart);
        });
    }
    {
        let app = std::rc::Rc::clone(app);
        settings.on_install_firmware(move || app.send(Event::InstallFirmware));
    }
}
