use num_traits::ToPrimitive;

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use slint::{ComponentHandle, SharedString};

use super::state::App;
use crate::config::Config;
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
                    tracing::info!("update {} available", release.tag_name);
                    let tag = release.tag_name.trim_start_matches('v').to_owned();
                    *pending
                        .lock()
                        .expect("release storage stays valid unless its holder panics") =
                        Some(release);
                    let _ = weak.upgrade_in_event_loop(move |w| {
                        let updates = w.global::<Updates>();
                        updates.set_version(tag.into());
                        updates.set_state(SharedString::new());
                        updates.set_busy(false);
                        updates.set_shown(true);
                    });
                }
                Ok(_) => tracing::info!("no newer release"),
                Err(error) => tracing::warn!(error = %error, "update check: {error}"),
            },
        );
}

fn install(window: &AppWindow, pending: &Pending, staging: PathBuf, log_path: PathBuf) {
    let Some(release) = pending
        .lock()
        .expect("release storage stays valid unless its holder panics")
        .clone()
    else {
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
                let percent = (fraction * 100.0)
                    .to_i32()
                    .expect("download progress is between zero and one");
                let mut shown = shown
                    .lock()
                    .expect("progress storage stays valid unless its holder panics");
                if *shown != percent {
                    *shown = percent;
                    drop(shown);
                    let _ = weak.upgrade_in_event_loop(move |w| {
                        w.global::<Updates>()
                            .set_state(format!("Downloading {percent}%").into());
                    });
                }
            };
            let package = update::Package::current();
            match update::download(&release, package, &staging, &progress)
                .and_then(|file| update::apply(package, &file, &staging, &log_path))
            {
                Ok(()) => {
                    tracing::info!("installing {}", release.tag_name);
                    quit();
                }
                Err(error) => {
                    tracing::error!(error = %error, "update failed: {error}");
                    let _ = weak.upgrade_in_event_loop(move |w| {
                        let updates = w.global::<Updates>();
                        updates.set_busy(false);
                        updates.set_state(format!("Update failed: {error}").into());
                    });
                }
            }
        });
}

pub(super) fn bind(window: &AppWindow, app: &Rc<App>, dir: &Path, config: &Config) {
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
        let pending = std::sync::Arc::clone(&pending);
        let app = std::rc::Rc::clone(app);
        updates.on_update_skip(move || {
            if let Some(tag) = pending
                .lock()
                .expect("release storage stays valid unless its holder panics")
                .as_ref()
                .map(|r| r.tag_name.clone())
            {
                tracing::info!(tag = %tag, "skipping {tag}");
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
        let pending = std::sync::Arc::clone(&pending);
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
