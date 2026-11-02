use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use super::{Msg, Options, PanelView};

pub(super) const PROBE: Duration = Duration::from_secs(10);
pub(super) const AFTER_QUIT: Duration = Duration::from_secs(20);
pub(super) const AFTER_ERROR: Duration = Duration::from_secs(30);
const IDLE: Duration = Duration::from_secs(3600);

pub(super) enum SessionEnd {
    SteamVrQuit,
    AppQuit,
    Unregistered,
}

pub(super) trait Runtime {
    fn running(&mut self) -> bool;

    fn session(
        &mut self,
        enabled: &mut bool,
        registered: &mut bool,
        rx: &Receiver<Msg>,
        view: &mut PanelView,
    ) -> Result<SessionEnd, String>;
}

pub(super) fn supervise(
    options: &Options,
    rx: &Receiver<Msg>,
    runtime: &mut impl Runtime,
    on_quit: impl Fn(),
) {
    supervise_with(options, rx, runtime, on_quit, Instant::now, |wait| {
        rx.recv_timeout(wait)
    });
}

fn supervise_with(
    options: &Options,
    rx: &Receiver<Msg>,
    runtime: &mut impl Runtime,
    on_quit: impl Fn(),
    now: impl Fn() -> Instant,
    mut receive: impl FnMut(Duration) -> Result<Msg, RecvTimeoutError>,
) {
    let mut enabled = options.enabled;
    let mut registered = options.registered;
    let mut view = PanelView::default();
    let mut next_probe = now();
    let mut last_error: Option<String> = None;
    tracing::info!(
        "{}",
        if enabled {
            "steamvr: waiting for SteamVR"
        } else {
            "steamvr: off"
        }
    );
    loop {
        let active = enabled || registered;
        if active && now() >= next_probe {
            next_probe = now() + PROBE;
            if runtime.running() {
                match runtime.session(&mut enabled, &mut registered, rx, &mut view) {
                    Ok(SessionEnd::SteamVrQuit) => {
                        if options.launched {
                            on_quit();
                            return;
                        }
                        next_probe = now() + AFTER_QUIT;
                    }
                    Ok(SessionEnd::AppQuit) => return,
                    Ok(SessionEnd::Unregistered) => {}
                    Err(error) => {
                        if last_error.as_deref() != Some(error.as_str()) {
                            tracing::warn!(error = %error, "steamvr: {error}");
                            last_error = Some(error);
                        }
                        next_probe = now() + AFTER_ERROR;
                    }
                }
            }
        }
        let wait = if enabled || registered {
            next_probe.saturating_duration_since(now())
        } else {
            IDLE
        };
        match receive(wait) {
            Ok(Msg::Quit) | Err(RecvTimeoutError::Disconnected) => return,
            Ok(Msg::Enable(on)) => {
                enabled = on;
                next_probe = now();
            }
            Ok(Msg::View(next)) => view = next,
            Err(RecvTimeoutError::Timeout) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;
    use std::sync::mpsc::{Sender, channel};

    #[derive(Default)]
    struct Fake {
        running: bool,
        probes: usize,
        sessions: usize,
        end: Option<fn() -> Result<SessionEnd, String>>,
        seen: Vec<PanelView>,
    }

    impl Runtime for Fake {
        fn running(&mut self) -> bool {
            self.probes += 1;
            self.running
        }

        fn session(
            &mut self,
            _enabled: &mut bool,
            _registered: &mut bool,
            _rx: &Receiver<Msg>,
            view: &mut PanelView,
        ) -> Result<SessionEnd, String> {
            self.sessions += 1;
            self.seen.push(view.clone());
            (self.end.expect("a session end"))()
        }
    }

    fn options(enabled: bool) -> Options {
        Options {
            enabled,
            registered: enabled,
            #[cfg(windows)]
            data_dir: std::path::PathBuf::new(),
            launched: false,
        }
    }

    fn view(bpm: u16) -> Msg {
        Msg::View(PanelView {
            connected: true,
            bpm,
            percent: "0.50".into(),
            status: "Pico on COM5".into(),
        })
    }

    fn feed(tx: &Sender<Msg>, messages: impl IntoIterator<Item = Msg>) {
        for message in messages {
            tx.send(message).unwrap();
        }
    }

    #[test]
    fn unregistering_returns_to_the_message_loop() {
        let (tx, rx) = channel();
        feed(&tx, [Msg::Quit]);
        let mut fake = Fake {
            running: true,
            end: Some(|| Ok(SessionEnd::Unregistered)),
            ..Fake::default()
        };
        supervise(&options(true), &rx, &mut fake, || {});
        assert_eq!(fake.sessions, 1);
    }

    #[test]
    fn readings_do_not_trigger_process_scans() {
        let (tx, rx) = channel();
        feed(&tx, (60..160).map(view));
        feed(&tx, [Msg::Quit]);
        let mut fake = Fake::default();
        supervise(&options(true), &rx, &mut fake, || {});
        assert_eq!(fake.probes, 1, "one probe at start, none per reading");
    }

    #[test]
    fn nothing_is_probed_while_steamvr_is_off() {
        let (tx, rx) = channel();
        feed(&tx, (60..70).map(view));
        feed(&tx, [Msg::Quit]);
        let mut fake = Fake::default();
        supervise(&options(false), &rx, &mut fake, || {});
        assert_eq!(fake.probes, 0);
    }

    #[test]
    fn switching_on_probes_at_once() {
        let (tx, rx) = channel();
        feed(&tx, [Msg::Enable(true), view(70), view(71), Msg::Quit]);
        let mut fake = Fake::default();
        supervise(&options(false), &rx, &mut fake, || {});
        assert_eq!(fake.probes, 1);
    }

    #[test]
    fn the_session_gets_the_latest_view() {
        let (tx, rx) = channel();
        feed(&tx, [view(70), view(88), Msg::Enable(true)]);
        let mut fake = Fake {
            running: true,
            end: Some(|| Ok(SessionEnd::AppQuit)),
            ..Fake::default()
        };
        supervise(&options(false), &rx, &mut fake, || {});
        assert_eq!(fake.sessions, 1);
        assert_eq!(fake.seen[0].bpm, 88);
        drop(tx);
    }

    #[test]
    fn a_steamvr_launch_ends_with_steamvr() {
        let (_tx, rx) = channel();
        let quit = Rc::new(Cell::new(false));
        let mut fake = Fake {
            running: true,
            end: Some(|| Ok(SessionEnd::SteamVrQuit)),
            ..Fake::default()
        };
        let flag = std::rc::Rc::clone(&quit);
        supervise(
            &Options {
                launched: true,
                ..options(true)
            },
            &rx,
            &mut fake,
            move || flag.set(true),
        );
        assert!(quit.get());
        assert_eq!(fake.sessions, 1);
    }

    #[test]
    fn a_failed_session_waits_before_trying_again() {
        let (_tx, rx) = channel();
        let mut fake = Fake {
            running: true,
            end: Some(|| Err("VRInitError_Init_HmdNotFound".into())),
            ..Fake::default()
        };
        let now = Cell::new(Instant::now());
        let mut waits = Vec::new();
        supervise_with(
            &options(true),
            &rx,
            &mut fake,
            || {},
            || now.get(),
            |wait| {
                waits.push(wait);
                now.set(now.get() + wait);
                if waits.len() == 1 {
                    Err(RecvTimeoutError::Timeout)
                } else {
                    Ok(Msg::Quit)
                }
            },
        );
        assert_eq!(fake.sessions, 2);
        assert_eq!(waits, [AFTER_ERROR, AFTER_ERROR]);
    }
}
