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
    let apps = vr.table("IVRApplications_007")?;
    let overlay = vr.table("IVROverlay_025")?;
    let system = vr.table("IVRSystem_022")?;
    let key = openvr::cstr(APP_KEY);
    let mut panel = super::panel::Panel::new()?;
    unsafe {
        let identify: openvr::IdentifyFn = apps.get(openvr::apps::IDENTIFY);
        identify(std::process::id(), key.as_ptr());
        let create: openvr::CreateDashboardFn = overlay.get(openvr::overlay::CREATE_DASHBOARD);
        let (mut main, mut thumb) = (0u64, 0u64);
        let error = create(
            openvr::cstr(OVERLAY_KEY).as_ptr(),
            openvr::cstr("Heartwire").as_ptr(),
            &mut main,
            &mut thumb,
        );
        if error != 0 {
            return Err(format!("creating the dashboard overlay failed ({error})"));
        }
        let from_file: openvr::SetFromFileFn = overlay.get(openvr::overlay::SET_FROM_FILE);
        from_file(thumb, openvr::cstr(&icon.display().to_string()).as_ptr());
        let set_width: openvr::SetWidthFn = overlay.get(openvr::overlay::SET_WIDTH);
        set_width(main, 1.5);
        let set_raw: openvr::SetRawFn = overlay.get(openvr::overlay::SET_RAW);
        let is_visible: openvr::IsVisibleFn = overlay.get(openvr::overlay::IS_VISIBLE);
        let poll_overlay: openvr::PollOverlayFn = overlay.get(openvr::overlay::POLL_EVENT);
        let poll_system: openvr::PollSystemFn = system.get(openvr::system::POLL_EVENT);
        let ack_quit: openvr::AckQuitFn = system.get(openvr::system::ACK_QUIT);
        let destroy: openvr::DestroyFn = overlay.get(openvr::overlay::DESTROY);
        let size = std::mem::size_of::<openvr::Event>() as u32;
        let mut stale = true;
        let end = loop {
            let mut event = openvr::Event::default();
            let mut quit = false;
            while poll_system(&mut event, size) {
                quit |= event.kind == openvr::EVENT_QUIT;
            }
            while poll_overlay(main, &mut event, size) {
                stale |= event.kind == openvr::EVENT_OVERLAY_SHOWN;
            }
            if quit {
                ack_quit();
                crate::log::write("steamvr: SteamVR is closing");
                break SessionEnd::SteamVrQuit;
            }
            if stale && (is_visible(main) || !panel.drawn()) {
                let (pixels, width, height) = panel.render(view);
                set_raw(main, pixels.as_ptr() as *mut _, width, height, 4);
                stale = false;
            }
            match rx.recv_timeout(Duration::from_millis(500)) {
                Ok(Msg::Quit) | Err(RecvTimeoutError::Disconnected) => break SessionEnd::AppQuit,
                Ok(Msg::Enable(on)) => {
                    *enabled = on;
                    if !on {
                        destroy(main);
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
        destroy(main);
        Ok(end)
    }
}
