mod bridge;
mod devices;
mod instance;
mod settings;
pub(crate) mod taskbar;
mod updates;
mod welcome;

use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

use slint::ComponentHandle;

use crate::config::{self, Config, Store};
use crate::engine::{self, Event};
use crate::ui::{AppWindow, Navigation};
use crate::version::{self, Channel};
use crate::{log, platform, steamvr, update};

pub const TITLE: &str = "Heartwire";

fn has_flag(flag: &str) -> bool {
    std::env::args().any(|a| a == flag)
}

fn select_backend(minimized: bool) {
    let selected = slint::BackendSelector::new()
        .backend_name("winit".into())
        .renderer_name("software".into())
        .with_winit_window_attributes_hook(move |attributes| attributes.with_active(!minimized))
        .select();
    if let Err(error) = selected {
        log::write(&format!("backend: {error}"));
    }
}

fn start_steamvr(config: &Config, dir: &Path, launched: bool) -> steamvr::Handle {
    steamvr::start(
        steamvr::Options {
            enabled: config.steamvr_autostart,
            registered: config.steamvr_registered,
            data_dir: dir.to_path_buf(),
            launched,
        },
        settings::remember_steamvr_registration,
        || {
            let _ = slint::invoke_from_event_loop(|| {
                let _ = slint::quit_event_loop();
            });
        },
    )
}

fn show(window: &AppWindow, minimized: bool) -> bool {
    if let Err(error) = window.show() {
        log::write(&format!("show window: {error}"));
        return false;
    }
    if minimized {
        let weak = window.as_weak();
        slint::Timer::single_shot(Duration::ZERO, move || {
            if let Some(w) = weak.upgrade() {
                w.window().set_minimized(true);
            }
        });
    }
    true
}

pub fn run() {
    let launched_by_steamvr = has_flag(steamvr::LAUNCH_FLAG);
    let minimized = launched_by_steamvr || has_flag(platform::MINIMIZED_FLAG);
    let dir = config::data_dir();
    let Some(instance) = instance::acquire(&dir) else {
        if !launched_by_steamvr {
            platform::focus_existing(TITLE);
        }
        return;
    };
    log::init(&dir);
    log::write(&format!(
        "Heartwire {} ({}) {} starting",
        version::VERSION,
        Channel::current().name(),
        update::rid()
    ));
    platform::refresh_autostart();
    select_backend(minimized);

    let store = Store::new(&dir);
    let loaded = store.load();
    let config = loaded.config;
    let window = match AppWindow::new() {
        Ok(window) => window,
        Err(error) => {
            log::write(&format!("window: {error}"));
            return;
        }
    };
    settings::load(&window, &config);
    taskbar::install(&window);

    let (tx, rx) = mpsc::channel();
    let vr = start_steamvr(&config, &dir, launched_by_steamvr);
    let app = settings::App::new(config.clone(), store, tx.clone());
    let bridge = bridge::Bridge::new(&window, app.shared(), vr.sender());
    let engine_tx = tx.clone();
    let engine_config = config.clone();
    let engine = std::thread::Builder::new()
        .name("engine".into())
        .stack_size(256 * 1024)
        .spawn(move || engine::run(engine_config, rx, engine_tx, bridge))
        .expect("engine thread");

    settings::bind(&window, &app, vr.sender());
    updates::bind(&window, &app, &dir, &config);
    welcome::offer(&window, &app, loaded.hr_osc);
    window
        .global::<Navigation>()
        .on_open_link(|url| platform::open_url(&url));

    if show(&window, minimized)
        && let Err(error) = slint::run_event_loop()
    {
        log::write(&format!("event loop: {error}"));
    }
    let _ = window.hide();
    app.flush();
    let _ = tx.send(Event::Quit);
    let _ = engine.join();
    drop(vr);
    log::write("stopped");
    drop(instance);
}
