use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use slint::{ComponentHandle, SharedString};

use super::settings::App;
use crate::config::Config;
use crate::log;
use crate::ui::{AppWindow, Updates};
use crate::update::{self, Release};
use crate::version::{self, Channel};

type Pending = Arc<Mutex<Option<Release>>>;

fn quit() {
    let _ = slint::invoke_from_event_loop(|| {
        let _ = slint::quit_event_loop();
    });
}

fn check(window: &AppWindow, pending: Pending, skipped: String) {
    let weak = window.as_weak();
    let _ = std::thread::Builder::new()
        .name("update-check".into())
        .spawn(
            move || match update::check(version::VERSION, Channel::current()) {
                Ok(Some(release)) if release.tag_name != skipped => {
                    log::write(&format!("update {} available", release.tag_name));
                    let tag = release.tag_name.trim_start_matches('v').to_owned();
                    *pending.lock().unwrap() = Some(release);
                    let _ = weak.upgrade_in_event_loop(move |w| {
                        let updates = w.global::<Updates>();
                        updates.set_version(tag.into());
                        updates.set_state(SharedString::new());
                        updates.set_busy(false);
                        updates.set_shown(true);
                    });
                }
                Ok(_) => log::write("no newer release"),
                Err(error) => log::write(&format!("update check: {error}")),
            },
        );
}

fn install(window: &AppWindow, pending: &Pending, staging: PathBuf, log_path: PathBuf) {
    let Some(release) = pending.lock().unwrap().clone() else {
        return;
    };
    let updates = window.global::<Updates>();
    updates.set_busy(true);
    updates.set_state("Downloading...".into());
    let weak = window.as_weak();
    let _ = std::thread::Builder::new()
        .name("update".into())
        .spawn(move || {
            let shown = Mutex::new(-1i32);
            let progress = |fraction: f32| {
                let percent = (fraction * 100.0) as i32;
                let mut shown = shown.lock().unwrap();
                if *shown != percent {
                    *shown = percent;
                    let _ = weak.upgrade_in_event_loop(move |w| {
                        w.global::<Updates>()
                            .set_state(format!("Downloading {percent}%").into());
                    });
                }
            };
            match update::download(&release, &staging, &progress)
                .and_then(|archive| update::apply(&archive, &staging, &log_path))
            {
                Ok(()) => {
                    log::write(&format!("installing {}", release.tag_name));
                    quit();
                }
                Err(error) => {
                    log::write(&format!("update failed: {error}"));
                    let _ = weak.upgrade_in_event_loop(move |w| {
                        let updates = w.global::<Updates>();
                        updates.set_busy(false);
                        updates.set_state(format!("Update failed: {error}").into());
                    });
                }
            }
        });
}

pub fn bind(window: &AppWindow, app: &Rc<App>, dir: &Path, config: &Config) {
    let pending: Pending = Arc::default();
    let updates = window.global::<Updates>();
    {
        let weak = window.as_weak();
        updates.on_update_later(move || {
            if let Some(w) = weak.upgrade() {
                w.global::<Updates>().set_shown(false);
            }
        });
    }
    {
        let weak = window.as_weak();
        let pending = pending.clone();
        let app = app.clone();
        updates.on_update_skip(move || {
            if let Some(tag) = pending.lock().unwrap().as_ref().map(|r| r.tag_name.clone()) {
                log::write(&format!("skipping {tag}"));
                app.edit(Duration::ZERO, |c| {
                    c.skipped_update = tag;
                    true
                });
            }
            if let Some(w) = weak.upgrade() {
                w.global::<Updates>().set_shown(false);
            }
        });
    }
    {
        let weak = window.as_weak();
        let pending = pending.clone();
        let staging = dir.join("update");
        let log_path = dir.join("heartwire.log");
        updates.on_update_now(move || {
            if let Some(w) = weak.upgrade() {
                install(&w, &pending, staging.clone(), log_path.clone());
            }
        });
    }
    if config.check_updates && Channel::current() != Channel::Dev {
        check(window, pending, config.skipped_update.clone());
    }
}
