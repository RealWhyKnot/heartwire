use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use super::devices;
use crate::config::{Config, Service, Store};
use crate::engine::{self, Event};
use crate::sources::Device;
use crate::ui::{AppWindow, HeartRate, Navigation, Settings, Updates};
use crate::{log, platform, steamvr, version};

const TYPING_PAUSE: Duration = Duration::from_millis(500);

pub struct Shared {
    pub config: Mutex<Config>,
    pub devices: Mutex<Arc<[Device]>>,
}

impl Shared {
    pub fn pinned_device(&self) -> String {
        self.config.lock().unwrap().bluetooth_device.clone()
    }
}

pub struct App {
    shared: Arc<Shared>,
    store: Store,
    tx: Sender<Event>,
    save_timer: slint::Timer,
}

thread_local! {
    static APP: RefCell<Option<Rc<App>>> = const { RefCell::new(None) };
}

impl App {
    pub fn new(config: Config, store: Store, tx: Sender<Event>) -> Rc<App> {
        let app = Rc::new(App {
            shared: Arc::new(Shared {
                config: Mutex::new(config),
                devices: Mutex::new(Arc::default()),
            }),
            store,
            tx,
            save_timer: slint::Timer::default(),
        });
        APP.with(|slot| *slot.borrow_mut() = Some(app.clone()));
        app
    }

    pub fn shared(&self) -> Arc<Shared> {
        self.shared.clone()
    }

    pub fn send(&self, event: Event) {
        let _ = self.tx.send(event);
    }

    pub fn edit(self: &Rc<Self>, delay: Duration, change: impl FnOnce(&mut Config) -> bool) {
        if change(&mut self.shared.config.lock().unwrap()) {
            self.commit(delay);
        }
    }

    pub fn commit(self: &Rc<Self>, delay: Duration) {
        let app = self.clone();
        let save = move || {
            let config = app.shared.config.lock().unwrap().clone();
            app.store.save(&config);
            app.send(Event::Config(Box::new(config)));
        };
        if delay.is_zero() {
            self.save_timer.stop();
            save();
        } else {
            self.save_timer
                .start(slint::TimerMode::SingleShot, delay, save);
        }
    }

    pub fn flush(&self) {
        self.save_timer.stop();
        self.store.save(&self.shared.config.lock().unwrap());
    }
}

pub fn remember_steamvr_registration(state: bool) {
    let _ = slint::invoke_from_event_loop(move || {
        APP.with(|slot| {
            if let Some(app) = slot.borrow().as_ref() {
                app.edit(Duration::ZERO, |c| {
                    let changed = c.steamvr_registered != state;
                    c.steamvr_registered = state;
                    changed
                });
            }
        });
    });
}

pub fn apply_setting(config: &mut Config, key: &str, value: &str) -> bool {
    let value = value.trim();
    let number = || value.parse::<u32>().ok().filter(|n| *n > 0);
    let port = || value.parse::<u16>().ok().filter(|n| *n > 0);
    let text = |field: &mut String| {
        *field = value.to_owned();
        true
    };
    match key {
        "connected_timeout" => number().map(|n| config.connected_timeout = n).is_some(),
        "max_heart_rate" => number().map(|n| config.max_heart_rate = n).is_some(),
        "http_server_port" => port().map(|n| config.http_server_port = n).is_some(),
        "osc_client_port" => port().map(|n| config.osc_client_port = n).is_some(),
        "osc_client_host" => text(&mut config.osc_client_host),
        "osc_path_connected" => text(&mut config.osc_path_connected),
        "osc_path_percent" => text(&mut config.osc_path_percent),
        "stromno_widget_id" => text(&mut config.stromno_widget_id),
        "pico_strap_name" => text(&mut config.pico_strap_name),
        _ => false,
    }
}

pub fn load(window: &AppWindow, config: &Config) {
    let settings = window.global::<Settings>();
    let services: Vec<SharedString> = Service::ALL.iter().map(|s| s.label().into()).collect();
    settings.set_services(ModelRc::new(VecModel::from(services)));
    settings.set_service(config.service_type.index() as i32);
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

pub fn bind(window: &AppWindow, app: &Rc<App>, vr: Sender<steamvr::Msg>) {
    let settings = window.global::<Settings>();
    {
        let app = app.clone();
        settings.on_setting(move |key, value| {
            app.edit(TYPING_PAUSE, |c| apply_setting(c, &key, &value));
        });
    }
    {
        let app = app.clone();
        let weak = window.as_weak();
        settings.on_service_picked(move |index| {
            let service = Service::ALL
                .get(index as usize)
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
        let app = app.clone();
        let weak = window.as_weak();
        settings.on_device_picked(move |address| {
            app.edit(Duration::ZERO, |c| {
                let changed = c.bluetooth_device != address.as_str();
                c.bluetooth_device = address.to_string();
                changed
            });
            if let Some(w) = weak.upgrade() {
                let shared = app.shared();
                let found = shared.devices.lock().unwrap().clone();
                w.global::<HeartRate>()
                    .set_devices(devices::model(&found, &shared.pinned_device()));
            }
        });
    }
    {
        let weak = window.as_weak();
        settings.on_autostart_toggled(move |on| {
            if let Err(error) = platform::set_autostart(on) {
                log::write(&format!("autostart: {error}"));
            }
            if let Some(w) = weak.upgrade() {
                w.global::<Settings>()
                    .set_autostart(platform::autostart_enabled());
            }
        });
    }
    {
        let app = app.clone();
        settings.on_steamvr_toggled(move |on| {
            app.edit(Duration::ZERO, |c| {
                c.steamvr_autostart = on;
                true
            });
            let _ = vr.send(steamvr::Msg::Enable(on));
        });
    }
    {
        let app = app.clone();
        settings.on_updates_toggled(move |on| {
            app.edit(Duration::ZERO, |c| {
                c.check_updates = on;
                true
            });
        });
    }
    {
        let app = app.clone();
        settings.on_restart_source(move || {
            app.commit(Duration::ZERO);
            app.send(Event::Restart);
        });
    }
    {
        let app = app.clone();
        settings.on_install_firmware(move || app.send(Event::InstallFirmware));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_must_be_positive() {
        let mut c = Config::default();
        assert!(apply_setting(&mut c, "max_heart_rate", " 185 "));
        assert_eq!(c.max_heart_rate, 185);
        assert!(!apply_setting(&mut c, "max_heart_rate", "0"));
        assert!(!apply_setting(&mut c, "max_heart_rate", "fast"));
        assert_eq!(c.max_heart_rate, 185);
    }

    #[test]
    fn ports_must_fit_in_sixteen_bits() {
        let mut c = Config::default();
        assert!(apply_setting(&mut c, "osc_client_port", "9001"));
        assert_eq!(c.osc_client_port, 9001);
        assert!(!apply_setting(&mut c, "osc_client_port", "70000"));
        assert!(!apply_setting(&mut c, "http_server_port", "0"));
        assert_eq!(c.http_server_port, 8080);
    }

    #[test]
    fn text_fields_are_trimmed() {
        let mut c = Config::default();
        assert!(apply_setting(&mut c, "osc_client_host", "  192.168.1.5 "));
        assert_eq!(c.osc_client_host, "192.168.1.5");
        assert!(apply_setting(&mut c, "pico_strap_name", " Polar "));
        assert_eq!(c.pico_strap_name, "Polar");
    }

    #[test]
    fn unknown_keys_change_nothing() {
        let mut c = Config::default();
        assert!(!apply_setting(&mut c, "volume", "11"));
        assert_eq!(c, Config::default());
    }
}
