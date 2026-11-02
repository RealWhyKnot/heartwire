use std::io::{ErrorKind, Read, Write};
use std::time::{Duration, Instant};

use super::lines::{LineBuffer, PicoLine, note_text, parse_pico_line};
use crate::sources::Context;

pub(super) const SILENT: &str = "The Pico is silent. Install the firmware in Settings > Pico";

struct Link<'a> {
    ctx: &'a Context,
    alive: bool,
    poked: bool,
    streaming: bool,
    strap: Option<String>,
    waiting_since: Instant,
}

impl Link<'_> {
    fn line(&mut self, raw: &[u8], now: &impl Fn() -> Instant) {
        match parse_pico_line(raw) {
            PicoLine::Reading(bpm) => {
                self.alive = true;
                if !self.streaming {
                    self.streaming = true;
                    self.ctx.status(match &self.strap {
                        Some(name) => format!("Pico: {name}"),
                        None => note_text("subscribed"),
                    });
                }
                self.ctx.reading(bpm);
            }
            PicoLine::Note(note) => {
                self.alive = true;
                self.streaming = false;
                if let Some(name) = note.strip_prefix("found ") {
                    self.strap = Some(name.to_owned());
                }
                let _ = crate::log::write_changed(&format!("pico: {note}"));
                self.ctx.status(note_text(note));
            }
            PicoLine::Repl => {
                self.alive = false;
                self.poked = false;
                self.waiting_since = now();
            }
            PicoLine::Other => {}
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct Timing {
    pub quiet: Duration,
    pub backlog: Duration,
}

pub(super) fn pump<P: Read + Write + ?Sized>(
    ctx: &Context,
    port: &str,
    io: &mut P,
    timing: Timing,
) -> std::io::Result<()> {
    pump_with(ctx, port, io, timing, Instant::now, std::thread::sleep)
}

fn pump_with<P: Read + Write + ?Sized>(
    ctx: &Context,
    port: &str,
    io: &mut P,
    timing: Timing,
    now: impl Fn() -> Instant,
    wait: impl Fn(Duration),
) -> std::io::Result<()> {
    let Timing { quiet, backlog } = timing;
    let mut lines = LineBuffer::default();
    let mut buf = [0u8; 256];
    let opened = now();
    let mut link = Link {
        ctx,
        alive: false,
        poked: false,
        streaming: false,
        strap: None,
        waiting_since: now(),
    };
    while !ctx.stopped() {
        if !link.alive && now().duration_since(link.waiting_since) >= quiet {
            if link.poked {
                ctx.status(SILENT);
            } else {
                tracing::info!(port = %port, "pico {port} is quiet, asking it to restart");
                let _ = io.write_all(b"\x04");
                link.poked = true;
            }
            link.waiting_since = now();
        }
        match io.read(&mut buf) {
            Ok(0) => wait(Duration::from_millis(100)),
            Ok(_) if now().duration_since(opened) < backlog => {}
            Ok(n) => lines.feed(&buf[..n], |line| link.line(line, &now)),
            Err(error)
                if matches!(
                    error.kind(),
                    ErrorKind::TimedOut | ErrorKind::WouldBlock | ErrorKind::Interrupted
                ) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Event;
    use crate::sources::Stopper;
    use std::cell::Cell;
    use std::collections::VecDeque;
    use std::rc::Rc;

    enum Output {
        Bytes(&'static [u8]),
        Quiet(Duration),
    }

    struct Board {
        output: VecDeque<Output>,
        input: Vec<u8>,
        now: Rc<Cell<Instant>>,
        stop: Stopper,
    }

    impl Read for Board {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            match self.output.pop_front() {
                Some(Output::Bytes(chunk)) => {
                    buf[..chunk.len()].copy_from_slice(chunk);
                    Ok(chunk.len())
                }
                Some(Output::Quiet(duration)) => {
                    self.now.set(self.now.get() + duration);
                    Err(ErrorKind::TimedOut.into())
                }
                None => {
                    self.stop.stop();
                    Err(ErrorKind::TimedOut.into())
                }
            }
        }
    }

    impl Write for Board {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.input.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    struct Observed {
        events: Vec<Event>,
        input: Vec<u8>,
    }

    impl Observed {
        fn statuses(&self) -> Vec<&str> {
            self.events
                .iter()
                .filter_map(|event| match event {
                    Event::Status { text, .. } => Some(text.as_str()),
                    Event::Reading { .. }
                    | Event::Devices { .. }
                    | Event::Quit
                    | Event::Config(_)
                    | Event::Restart
                    | Event::InstallFirmware => None,
                })
                .collect()
        }

        fn readings(&self) -> Vec<u16> {
            self.events.iter().filter_map(Event::reading).collect()
        }
    }

    fn run(output: impl IntoIterator<Item = Output>, timing: Timing) -> Observed {
        let (ctx, rx, stop) = Context::test();
        let now = Rc::new(Cell::new(Instant::now()));
        let mut board = Board {
            output: output.into_iter().collect(),
            input: Vec::new(),
            now: Rc::clone(&now),
            stop,
        };
        pump_with(
            &ctx,
            "COM9",
            &mut board,
            timing,
            || now.get(),
            |duration| {
                now.set(now.get() + duration);
            },
        )
        .unwrap();
        Observed {
            events: rx.try_iter().collect(),
            input: board.input,
        }
    }

    const TIMING: Timing = Timing {
        quiet: Duration::from_millis(30),
        backlog: Duration::ZERO,
    };

    #[test]
    fn readings_after_a_note_bring_the_status_back() {
        let observed = run(
            [
                Output::Bytes(b"nd, rescanning\r\n# found COOSPO HW807\r\n# subscribed\r\n"),
                Output::Bytes(b"# no skin contact\r\n"),
                Output::Bytes(b"72\r\n73\r\n"),
            ],
            TIMING,
        );
        assert_eq!(observed.readings(), [72, 73]);
        assert_eq!(
            observed.statuses(),
            [
                "Pico found COOSPO HW807",
                "Pico connected to the strap",
                "Pico: no skin contact",
                "Pico: COOSPO HW807",
            ]
        );
        assert_eq!(observed.input.len(), 0);
    }

    #[test]
    fn a_board_that_never_speaks_is_nudged_once_then_reported() {
        let observed = run(
            [
                Output::Quiet(TIMING.quiet),
                Output::Quiet(TIMING.quiet),
                Output::Quiet(TIMING.quiet),
            ],
            TIMING,
        );
        assert_eq!(observed.input, b"\x04");
        assert_eq!(observed.statuses(), [SILENT]);
    }

    #[test]
    fn firmware_that_goes_quiet_is_left_alone() {
        let observed = run(
            [
                Output::Bytes(b"\n# subscribed\r\n# no skin contact\r\n"),
                Output::Quiet(Duration::from_millis(200)),
            ],
            TIMING,
        );
        assert_eq!(observed.input.len(), 0, "no Ctrl-D for a running firmware");
        assert_eq!(
            observed.statuses(),
            ["Pico connected to the strap", "Pico: no skin contact"]
        );
    }

    #[test]
    fn a_board_back_at_the_repl_is_nudged_again() {
        let observed = run([
            Output::Bytes(b"\n# subscribed\r\n"),
            Output::Bytes(b"Traceback\r\nMicroPython v1.28.0\r\nType \"help()\" for more information.\r\n"),
            Output::Quiet(TIMING.quiet),
            Output::Quiet(TIMING.quiet),
        ], TIMING);
        assert_eq!(observed.input, b"\x04");
        assert!(observed.statuses().contains(&SILENT));
    }

    #[test]
    fn the_backlog_read_right_after_opening_is_ignored() {
        let observed = run(
            [
                Output::Bytes(b"\n# no strap found# no strap found, rescanning\r\n99\r\n"),
                Output::Quiet(Duration::from_millis(150)),
                Output::Bytes(b"7"),
                Output::Bytes(b"2\r\n# subscribed\r\n73\r\n"),
            ],
            Timing {
                quiet: Duration::from_secs(60),
                backlog: Duration::from_millis(100),
            },
        );
        assert_eq!(observed.statuses(), ["Pico connected to the strap"]);
        assert_eq!(
            observed.readings(),
            [73],
            "the stale 99 and the partial 72 are dropped"
        );
    }
}
