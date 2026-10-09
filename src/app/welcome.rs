use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use slint::ComponentHandle;

use super::settings::{self, App};
use crate::config::{Config, read_hr_osc};
use crate::log;
use crate::ui::{AppWindow, Welcome};

pub fn carry_over(current: &Config, imported: Config) -> Config {
    Config {
        steamvr_autostart: current.steamvr_autostart,
        steamvr_registered: current.steamvr_registered,
        check_updates: current.check_updates,
        ..imported
    }
}

pub fn offer(window: &AppWindow, app: &Rc<App>, hr_osc: Option<PathBuf>) {
    let Some(path) = hr_osc else {
        return;
    };
    let welcome = window.global::<Welcome>();
    welcome.set_shown(true);
    {
        let weak = window.as_weak();
        let app = app.clone();
        welcome.on_import_hr_osc(move || {
            let Some(w) = weak.upgrade() else {
                return;
            };
            match read_hr_osc(&path) {
                Some(imported) => {
                    let mut applied = None;
                    app.edit(Duration::ZERO, |current| {
                        *current = carry_over(current, imported);
                        applied = Some(current.clone());
                        true
                    });
                    if let Some(config) = applied {
                        settings::load(&w, &config);
                    }
                    log::write("imported settings from hr-osc");
                }
                None => log::write("hr-osc settings could not be read"),
            }
            w.global::<Welcome>().set_shown(false);
        });
    }
    {
        let weak = window.as_weak();
        welcome.on_skip(move || {
            log::write("kept the default settings");
            if let Some(w) = weak.upgrade() {
                w.global::<Welcome>().set_shown(false);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Service;

    #[test]
    fn import_takes_hr_osc_values_and_keeps_heartwire_only_switches() {
        let current = Config {
            steamvr_autostart: true,
            check_updates: false,
            ..Config::default()
        };
        let imported = Config::from_json(
            r#"{"service_type":"http","max_heart_rate":230,"connected_timeout":60}"#,
        );
        let merged = carry_over(&current, imported);
        assert_eq!(merged.service_type, Service::Http);
        assert_eq!(merged.max_heart_rate, 230);
        assert_eq!(merged.connected_timeout, 60);
        assert!(merged.steamvr_autostart);
        assert!(!merged.check_updates);
    }
}
