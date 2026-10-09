use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::mpsc::Sender;
use std::time::Duration;

use crate::config::{Config, Store};
use crate::engine::Event;
use crate::sources::Device;

pub struct App {
    config: RefCell<Config>,
    devices: RefCell<Arc<[Device]>>,
    store: Store,
    tx: Sender<Event>,
    save_timer: slint::Timer,
}

thread_local! {
    static APP: RefCell<Option<Rc<App>>> = const { RefCell::new(None) };
}

impl App {
    pub fn install(config: Config, store: Store, tx: Sender<Event>) -> Rc<App> {
        let app = Rc::new(App {
            config: RefCell::new(config),
            devices: RefCell::new(Arc::default()),
            store,
            tx,
            save_timer: slint::Timer::default(),
        });
        APP.with(|slot| *slot.borrow_mut() = Some(app.clone()));
        app
    }

    pub fn with(f: impl FnOnce(&Rc<App>)) {
        if let Some(app) = APP.with(|slot| slot.borrow().clone()) {
            f(&app);
        }
    }

    pub fn pinned_device(&self) -> String {
        self.config.borrow().bluetooth_device.clone()
    }

    pub fn devices(&self) -> Arc<[Device]> {
        self.devices.borrow().clone()
    }

    pub fn set_devices(&self, list: Arc<[Device]>) -> bool {
        let mut current = self.devices.borrow_mut();
        if Arc::ptr_eq(&current, &list) {
            return false;
        }
        *current = list;
        true
    }

    pub fn send(&self, event: Event) {
        let _ = self.tx.send(event);
    }

    pub fn edit(self: &Rc<Self>, delay: Duration, change: impl FnOnce(&mut Config) -> bool) {
        let changed = change(&mut self.config.borrow_mut());
        if changed {
            self.commit(delay);
        }
    }

    pub fn commit(self: &Rc<Self>, delay: Duration) {
        let app = self.clone();
        let save = move || {
            let config = app.config.borrow().clone();
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
        self.store.save(&self.config.borrow());
    }
}

pub fn remember_steamvr_registration(state: bool) {
    let _ = slint::invoke_from_event_loop(move || {
        App::with(|app| {
            app.edit(Duration::ZERO, |c| {
                let changed = c.steamvr_registered != state;
                c.steamvr_registered = state;
                changed
            });
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("heartwire-state-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn only_a_real_change_is_saved_and_sent_to_the_engine() {
        let dir = scratch("edit");
        let (tx, rx) = channel();
        let app = App::install(Config::default(), Store::new(&dir), tx);
        app.edit(Duration::ZERO, |c| c.set("max_heart_rate", "200"));
        assert!(rx.try_recv().is_err(), "unchanged");
        assert!(!dir.join("config.json").exists(), "nothing written");
        app.edit(Duration::ZERO, |c| c.set("max_heart_rate", " 185 "));
        match rx.try_recv() {
            Ok(Event::Config(config)) => assert_eq!(config.max_heart_rate, 185),
            _ => panic!("the engine hears about the change"),
        }
        assert_eq!(Store::new(&dir).load().config.max_heart_rate, 185);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_device_list_counts_as_new_only_when_it_is_a_new_list() {
        let (tx, _rx) = channel();
        let app = App::install(Config::default(), Store::new(&scratch("devices")), tx);
        let list: Arc<[Device]> = vec![Device {
            name: "Polar H10".into(),
            address: "C1:22:33:44:55:66".into(),
        }]
        .into();
        assert!(app.set_devices(list.clone()));
        assert!(!app.set_devices(list.clone()));
        assert_eq!(app.devices().len(), 1);
    }

    #[test]
    fn the_installed_app_is_reachable_from_event_loop_callbacks() {
        let (tx, _rx) = channel();
        let config = Config {
            bluetooth_device: "C0:FF:EE:00:11:22".into(),
            ..Config::default()
        };
        let _app = App::install(config, Store::new(&scratch("with")), tx);
        let mut seen = String::new();
        App::with(|app| seen = app.pinned_device());
        assert_eq!(seen, "C0:FF:EE:00:11:22");
    }
}
