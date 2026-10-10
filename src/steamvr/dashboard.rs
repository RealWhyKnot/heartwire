use std::path::Path;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;

use super::openvr;
use super::registration::{apply_registration, write_files};
use super::supervisor::SessionEnd;
use super::{APP_KEY, Msg, PanelView};

const OVERLAY_KEY: &str = "dev.whyknot.heartwire.panel";

pub(super) fn session(
    enabled: &mut bool,
    registered: &mut bool,
    on_registered: &dyn Fn(bool),
    data_dir: &Path,
    rx: &Receiver<Msg>,
    view: &mut PanelView,
) -> Result<SessionEnd, String> {
    let needs_registration = !*enabled || !*registered;
    let mut note = |state: bool| {
        if *registered != state {
            *registered = state;
            on_registered(state);
        }
    };
    if needs_registration {
        let mut utility = openvr::load_runtime()?;
        utility.start(openvr::APP_UTILITY)?;
        apply_registration(&utility, *enabled, data_dir)?;
        note(*enabled);
        crate::log::write(if *enabled {
            "steamvr: added to SteamVR start-up"
        } else {
            "steamvr: removed from SteamVR start-up"
        });
        if !*enabled {
            return Ok(SessionEnd::Unregistered);
        }
    }
    let mut vr = openvr::load_runtime()?;
    vr.start(openvr::APP_OVERLAY)?;
    crate::log::write("steamvr: dashboard connected");
    let (_, icon) = write_files(data_dir)?;
    let apps = vr.applications()?;
    let overlay = vr.overlay()?;
    let system = vr.system()?;
    let mut panel = super::panel::Panel::new()?;
    apps.identify(std::process::id(), APP_KEY);
    let (main, thumb) = overlay
        .create_dashboard(OVERLAY_KEY, "Heartwire")
        .map_err(|error| format!("creating the dashboard overlay failed ({error})"))?;
    overlay.set_from_file(thumb, &icon);
    overlay.set_width(main, 1.5);
    let mut stale = true;
    let end = loop {
        let mut event = openvr::Event::default();
        let mut quit = false;
        while system.poll(&mut event) {
            quit |= event.kind == openvr::EVENT_QUIT;
        }
        while overlay.poll(main, &mut event) {
            stale |= event.kind == openvr::EVENT_OVERLAY_SHOWN;
        }
        if quit {
            system.ack_quit();
            crate::log::write("steamvr: SteamVR is closing");
            break SessionEnd::SteamVrQuit;
        }
        if stale && (overlay.is_visible(main) || !panel.drawn()) {
            let (pixels, width, height) = panel.render(view);
            overlay.set_rgba(main, pixels, width, height);
            stale = false;
        }
        match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(Msg::Quit) | Err(RecvTimeoutError::Disconnected) => break SessionEnd::AppQuit,
            Ok(Msg::Enable(on)) => {
                *enabled = on;
                if !on {
                    overlay.destroy(main);
                    apply_registration(&vr, false, data_dir)?;
                    crate::log::write("steamvr: removed from SteamVR start-up");
                    note(false);
                    return Ok(SessionEnd::Unregistered);
                }
            }
            Ok(Msg::View(next)) => {
                stale |= next != *view;
                *view = next;
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
    };
    overlay.destroy(main);
    Ok(end)
}
