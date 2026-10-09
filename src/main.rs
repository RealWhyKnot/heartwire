#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod beat;
mod config;
mod engine;
mod hr;
mod log;
mod net;
mod osc;
mod platform;
mod sources;
#[cfg(test)]
mod ui_tests;
mod update;
mod version;

use std::fs::File;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use config::{Config, Service, Store};
use engine::{Event, View};
use sources::Device;
use version::Channel;

slint::include_modules!();

const TITLE: &str = "hr-osc-rust";

thread_local! {
    static BEAT: std::cell::RefCell<Option<Rc<beat::Beat>>> = const { std::cell::RefCell::new(None) };
}

struct Shared {
    config: Mutex<Config>,
    devices: Mutex<Vec<Device>>,
}

fn device_rows(devices: &[Device], pinned: &str) -> Vec<DeviceRow> {
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

fn show_devices(window: &AppWindow, shared: &Shared) {
    let pinned = shared.config.lock().unwrap().bluetooth_device.clone();
    let devices = shared.devices.lock().unwrap().clone();
    window.set_devices(ModelRc::new(VecModel::from(device_rows(&devices, &pinned))));
}

struct Bridge {
    window: slint::Weak<AppWindow>,
    shared: Arc<Shared>,
}

impl engine::Ui for Bridge {
    fn show(&self, view: &View) {
        let view = view.clone();
        let shared = self.shared.clone();
        let _ = self.window.upgrade_in_event_loop(move |w| {
            BEAT.with(|b| {
                if let Some(beat) = b.borrow().as_ref() {
                    beat.update(view.connected, view.bpm);
                }
            });
            w.set_connected(view.connected);
            w.set_bpm(i32::from(view.bpm));
            w.set_percent_text(format!("{:.2}", view.percent).into());
            w.set_status(view.status.as_str().into());
            w.set_notice(view.notice.as_str().into());
            let changed = {
                let mut devices = shared.devices.lock().unwrap();
                let changed = *devices != view.devices;
                *devices = view.devices;
                changed
            };
            if changed || w.get_devices().row_count() == 0 {
                show_devices(&w, &shared);
            }
        });
    }
}

use slint::Model;

fn load_window(window: &AppWindow, config: &Config) {
    let services: Vec<SharedString> = Service::ALL.iter().map(|s| s.label().into()).collect();
    window.set_services(ModelRc::new(VecModel::from(services)));
    window.set_service(config.service_type.index() as i32);
    window.set_connected_timeout(config.connected_timeout.to_string().into());
    window.set_http_port(config.http_server_port.to_string().into());
    window.set_widget_id(config.stromno_widget_id.as_str().into());
    window.set_osc_host(config.osc_client_host.as_str().into());
    window.set_osc_port(config.osc_client_port.to_string().into());
    window.set_path_connected(config.osc_path_connected.as_str().into());
    window.set_path_percent(config.osc_path_percent.as_str().into());
    window.set_max_heart_rate(config.max_heart_rate.to_string().into());
    window.set_pico_strap(config.pico_strap_name.as_str().into());
    window.set_check_updates(config.check_updates);
    window.set_autostart(platform::autostart_enabled());
    window.set_version(version::VERSION.into());
    window.set_percent_text(format!("{:.2}", engine::percent(0, config.max_heart_rate)).into());
}

fn apply_setting(config: &mut Config, key: &str, value: &str) -> bool {
    let number = || value.trim().parse::<u32>().ok().filter(|n| *n > 0);
    let port = || value.trim().parse::<u16>().ok().filter(|n| *n > 0);
    match key {
        "connected_timeout" => number().map(|n| config.connected_timeout = n).is_some(),
        "max_heart_rate" => number().map(|n| config.max_heart_rate = n).is_some(),
        "http_server_port" => port().map(|n| config.http_server_port = n).is_some(),
        "osc_client_port" => port().map(|n| config.osc_client_port = n).is_some(),
        "osc_client_host" => {
            config.osc_client_host = value.trim().to_owned();
            true
        }
        "osc_path_connected" => {
            config.osc_path_connected = value.trim().to_owned();
            true
        }
        "osc_path_percent" => {
            config.osc_path_percent = value.trim().to_owned();
            true
        }
        "stromno_widget_id" => {
            config.stromno_widget_id = value.trim().to_owned();
            true
        }
        "pico_strap_name" => {
            config.pico_strap_name = value.trim().to_owned();
            true
        }
        _ => false,
    }
}

struct App {
    shared: Arc<Shared>,
    store: Store,
    tx: Sender<Event>,
    save_timer: slint::Timer,
}

impl App {
    fn commit(self: &Rc<Self>, delay: Duration) {
        let app = self.clone();
        let save = move || {
            let config = app.shared.config.lock().unwrap().clone();
            app.store.save(&config);
            let _ = app.tx.send(Event::Config(Box::new(config)));
        };
        if delay.is_zero() {
            self.save_timer.stop();
            save();
        } else {
            self.save_timer
                .start(slint::TimerMode::SingleShot, delay, save);
        }
    }

    fn edit(self: &Rc<Self>, delay: Duration, change: impl FnOnce(&mut Config) -> bool) {
        let changed = change(&mut self.shared.config.lock().unwrap());
        if changed {
            self.commit(delay);
        }
    }
}

#[derive(Default)]
struct UpdateSlot {
    release: Option<update::Release>,
}

fn start_update_check(window: &AppWindow, slot: Arc<Mutex<UpdateSlot>>, skipped: String) {
    let weak = window.as_weak();
    let _ = std::thread::Builder::new()
        .name("update-check".into())
        .spawn(
            move || match update::check(version::VERSION, Channel::current()) {
                Ok(Some(release)) if release.tag_name != skipped => {
                    log::write(&format!("update {} available", release.tag_name));
                    let tag = release.tag_name.clone();
                    slot.lock().unwrap().release = Some(release);
                    let _ = weak.upgrade_in_event_loop(move |w| {
                        w.set_update_version(tag.trim_start_matches('v').into());
                        w.set_update_state(SharedString::new());
                        w.set_update_busy(false);
                        w.set_update_shown(true);
                    });
                }
                Ok(_) => log::write("no newer release"),
                Err(error) => log::write(&format!("update check: {error}")),
            },
        );
}

fn run_update(
    window: &AppWindow,
    slot: Arc<Mutex<UpdateSlot>>,
    staging: PathBuf,
    log_path: PathBuf,
) {
    let Some(release) = slot.lock().unwrap().release.clone() else {
        return;
    };
    window.set_update_busy(true);
    window.set_update_state("Downloading...".into());
    let weak = window.as_weak();
    let _ = std::thread::Builder::new()
        .name("update".into())
        .spawn(move || {
            let last = Mutex::new(-1i32);
            let progress = |fraction: f32| {
                let percent = (fraction * 100.0) as i32;
                let mut last = last.lock().unwrap();
                if *last != percent {
                    *last = percent;
                    let _ = weak.upgrade_in_event_loop(move |w| {
                        w.set_update_state(format!("Downloading {percent}%").into());
                    });
                }
            };
            let result = update::download(&release, &staging, &progress)
                .and_then(|archive| update::apply(&archive, &staging, &log_path));
            match result {
                Ok(()) => {
                    log::write(&format!("installing {}", release.tag_name));
                    let _ = slint::invoke_from_event_loop(|| {
                        let _ = slint::quit_event_loop();
                    });
                }
                Err(error) => {
                    log::write(&format!("update failed: {error}"));
                    let _ = weak.upgrade_in_event_loop(move |w| {
                        w.set_update_busy(false);
                        w.set_update_state(format!("Update failed: {error}").into());
                    });
                }
            }
        });
}

fn main() {
    let minimized = std::env::args().any(|a| a == platform::MINIMIZED_FLAG);
    let dir = config::data_dir();
    let _ = std::fs::create_dir_all(&dir);
    let lock = File::create(dir.join("hr-osc-rust.lock")).ok();
    if lock.as_ref().is_some_and(|f| f.try_lock().is_err()) {
        platform::focus_existing(TITLE);
        return;
    }
    log::init(&dir);
    log::write(&format!(
        "hr-osc-rust {} ({}) {} starting",
        version::VERSION,
        Channel::current().name(),
        update::rid()
    ));
    platform::refresh_autostart();

    let selected = slint::BackendSelector::new()
        .backend_name("winit".into())
        .renderer_name("software".into())
        .with_winit_window_attributes_hook(move |attributes| attributes.with_active(!minimized))
        .select();
    if let Err(error) = selected {
        log::write(&format!("backend: {error}"));
    }

    let store = Store::new(&dir);
    let config = store.load();
    let window = match AppWindow::new() {
        Ok(window) => window,
        Err(error) => {
            log::write(&format!("window: {error}"));
            return;
        }
    };
    load_window(&window, &config);
    BEAT.with(|b| *b.borrow_mut() = Some(beat::Beat::new(&window)));

    let shared = Arc::new(Shared {
        config: Mutex::new(config.clone()),
        devices: Mutex::new(Vec::new()),
    });
    let (tx, rx) = mpsc::channel();
    let bridge = Bridge {
        window: window.as_weak(),
        shared: shared.clone(),
    };
    let engine_tx = tx.clone();
    let engine_config = config.clone();
    let engine = std::thread::Builder::new()
        .name("engine".into())
        .stack_size(256 * 1024)
        .spawn(move || engine::run(engine_config, rx, engine_tx, bridge))
        .expect("engine thread");

    let app = Rc::new(App {
        shared: shared.clone(),
        store,
        tx: tx.clone(),
        save_timer: slint::Timer::default(),
    });

    {
        let app = app.clone();
        window.on_setting(move |key, value| {
            app.edit(Duration::from_millis(500), |c| {
                apply_setting(c, &key, &value)
            });
        });
    }
    {
        let app = app.clone();
        let weak = window.as_weak();
        window.on_service_picked(move |index| {
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
                w.set_tab("general".into());
            }
        });
    }
    {
        let app = app.clone();
        let weak = window.as_weak();
        let shared = shared.clone();
        window.on_device_picked(move |address| {
            app.edit(Duration::ZERO, |c| {
                let changed = c.bluetooth_device != address.as_str();
                c.bluetooth_device = address.to_string();
                changed
            });
            if let Some(w) = weak.upgrade() {
                show_devices(&w, &shared);
            }
        });
    }
    {
        let weak = window.as_weak();
        window.on_autostart_toggled(move |on| {
            if let Err(error) = platform::set_autostart(on) {
                log::write(&format!("autostart: {error}"));
            }
            if let Some(w) = weak.upgrade() {
                w.set_autostart(platform::autostart_enabled());
            }
        });
    }
    {
        let app = app.clone();
        window.on_updates_toggled(move |on| {
            app.edit(Duration::ZERO, |c| {
                c.check_updates = on;
                true
            });
        });
    }
    {
        let app = app.clone();
        window.on_restart_source(move || {
            app.commit(Duration::ZERO);
            let _ = app.tx.send(Event::Restart);
        });
    }
    {
        let tx = tx.clone();
        window.on_install_firmware(move || {
            let _ = tx.send(Event::InstallFirmware);
        });
    }
    window.on_open_link(|url| platform::open_url(&url));

    let slot = Arc::new(Mutex::new(UpdateSlot::default()));
    {
        let weak = window.as_weak();
        window.on_update_later(move || {
            if let Some(w) = weak.upgrade() {
                w.set_update_shown(false);
            }
        });
    }
    {
        let weak = window.as_weak();
        let slot = slot.clone();
        let app = app.clone();
        window.on_update_skip(move || {
            if let Some(tag) = slot
                .lock()
                .unwrap()
                .release
                .as_ref()
                .map(|r| r.tag_name.clone())
            {
                log::write(&format!("skipping {tag}"));
                app.edit(Duration::ZERO, |c| {
                    c.skipped_update = tag;
                    true
                });
            }
            if let Some(w) = weak.upgrade() {
                w.set_update_shown(false);
            }
        });
    }
    {
        let weak = window.as_weak();
        let slot = slot.clone();
        let staging = dir.join("update");
        let log_path = dir.join("hr-osc-rust.log");
        window.on_update_now(move || {
            if let Some(w) = weak.upgrade() {
                run_update(&w, slot.clone(), staging.clone(), log_path.clone());
            }
        });
    }
    if config.check_updates && Channel::current() != Channel::Dev {
        start_update_check(&window, slot, config.skipped_update.clone());
    }

    if let Err(error) = window.show() {
        log::write(&format!("show window: {error}"));
        return;
    }
    if minimized {
        let weak = window.as_weak();
        slint::Timer::single_shot(Duration::ZERO, move || {
            if let Some(w) = weak.upgrade() {
                w.window().set_minimized(true);
            }
        });
    }
    if let Err(error) = slint::run_event_loop() {
        log::write(&format!("event loop: {error}"));
    }
    let _ = window.hide();
    app.save_timer.stop();
    let config = shared.config.lock().unwrap().clone();
    app.store.save(&config);
    let _ = tx.send(Event::Quit);
    let _ = engine.join();
    log::write("stopped");
    drop(lock);
}
