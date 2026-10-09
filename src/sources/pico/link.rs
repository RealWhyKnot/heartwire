use std::io::{ErrorKind, Read, Write};
use std::time::{Duration, Instant};

use super::lines::{LineBuffer, PicoLine, note_text, parse_pico_line};
use crate::sources::Context;

pub const SILENT: &str = "The Pico is silent. Install the firmware in Settings > Pico";

struct Link<'a> {
    ctx: &'a Context,
    alive: bool,
    poked: bool,
    streaming: bool,
    strap: Option<String>,
    waiting_since: Instant,
}

impl Link<'_> {
    fn line(&mut self, raw: &[u8]) {
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
                self.waiting_since = Instant::now();
            }
            PicoLine::Other => {}
        }
    }
}

#[derive(Clone, Copy)]
pub struct Timing {
    pub quiet: Duration,
    pub backlog: Duration,
}

pub fn pump<P: Read + Write + ?Sized>(
    ctx: &Context,
    port: &str,
    io: &mut P,
    timing: Timing,
) -> std::io::Result<()> {
    let Timing { quiet, backlog } = timing;
    let mut lines = LineBuffer::default();
    let mut buf = [0u8; 256];
    let opened = Instant::now();
    let mut link = Link {
        ctx,
        alive: false,
        poked: false,
        streaming: false,
        strap: None,
        waiting_since: Instant::now(),
    };
    while !ctx.stopped() {
        if !link.alive && link.waiting_since.elapsed() >= quiet {
            if link.poked {
                ctx.status(SILENT);
            } else {
                crate::log::write(&format!("pico {port} is quiet, asking it to restart"));
                let _ = io.write_all(b"\x04");
                link.poked = true;
            }
            link.waiting_since = Instant::now();
        }
        match io.read(&mut buf) {
            Ok(0) => std::thread::sleep(Duration::from_millis(100)),
            Ok(_) if opened.elapsed() < backlog => {}
            Ok(n) => lines.feed(&buf[..n], |line| link.line(line)),
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
    use std::collections::VecDeque;
    use std::sync::mpsc::Receiver;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct Board {
        output: Arc<Mutex<VecDeque<Vec<u8>>>>,
        input: Arc<Mutex<Vec<u8>>>,
    }

    impl Board {
        fn says(&self, chunk: &[u8]) {
            self.output.lock().unwrap().push_back(chunk.to_vec());
        }
        fn heard(&self) -> Vec<u8> {
            self.input.lock().unwrap().clone()
        }
    }

    impl Read for Board {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let Some(chunk) = self.output.lock().unwrap().pop_front() else {
                std::thread::sleep(Duration::from_millis(2));
                return Err(ErrorKind::TimedOut.into());
            };
            buf[..chunk.len()].copy_from_slice(&chunk);
            Ok(chunk.len())
        }
    }

    impl Write for Board {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.input.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    enum Seen {
        Status(String),
        Reading(u16),
    }

    fn collect(rx: &Receiver<Event>, until: impl Fn(&[Seen]) -> bool) -> Vec<Seen> {
        let mut seen = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !until(&seen) {
            let left = deadline.saturating_duration_since(Instant::now());
            match rx.recv_timeout(left) {
                Ok(Event::Status { text, .. }) => seen.push(Seen::Status(text)),
                Ok(Event::Reading { bpm, .. }) => seen.push(Seen::Reading(bpm)),
                Ok(_) => {}
                Err(_) => panic!("timed out"),
            }
        }
        seen
    }

    fn statuses(seen: &[Seen]) -> Vec<&str> {
        seen.iter()
            .filter_map(|s| match s {
                Seen::Status(text) => Some(text.as_str()),
                Seen::Reading(_) => None,
            })
            .collect()
    }

    fn run(board: &Board, quiet: Duration) -> (Receiver<Event>, impl FnOnce()) {
        run_with(
            board,
            Timing {
                quiet,
                backlog: Duration::ZERO,
            },
        )
    }

    fn run_with(board: &Board, timing: Timing) -> (Receiver<Event>, impl FnOnce()) {
        let (ctx, rx, stop) = Context::test();
        let mut io = board.clone();
        let worker = std::thread::spawn(move || pump(&ctx, "COM9", &mut io, timing));
        let finish = move || {
            stop.stop();
            worker.join().unwrap().unwrap();
        };
        (rx, finish)
    }

    #[test]
    fn readings_after_a_note_bring_the_status_back() {
        let board = Board::default();
        board.says(b"nd, rescanning\r\n# found COOSPO HW807\r\n# subscribed\r\n");
        board.says(b"# no skin contact\r\n");
        board.says(b"72\r\n73\r\n");
        let (rx, finish) = run(&board, Duration::from_secs(60));
        let seen = collect(&rx, |s| {
            s.iter().filter(|e| matches!(e, Seen::Reading(_))).count() == 2
        });
        finish();
        let readings: Vec<u16> = seen
            .iter()
            .filter_map(|s| match s {
                Seen::Reading(bpm) => Some(*bpm),
                Seen::Status(_) => None,
            })
            .collect();
        assert_eq!(readings, [72, 73]);
        assert_eq!(
            statuses(&seen),
            [
                "Pico found COOSPO HW807",
                "Pico connected to the strap",
                "Pico: no skin contact",
                "Pico: COOSPO HW807",
            ]
        );
        assert!(board.heard().is_empty());
    }

    #[test]
    fn a_board_that_never_speaks_is_nudged_once_then_reported() {
        let board = Board::default();
        let (rx, finish) = run(&board, Duration::from_millis(30));
        let seen = collect(&rx, |s| statuses(s).contains(&SILENT));
        finish();
        assert_eq!(board.heard(), b"\x04");
        assert_eq!(statuses(&seen), [SILENT]);
    }

    #[test]
    fn firmware_that_goes_quiet_is_left_alone() {
        let board = Board::default();
        board.says(b"\n# subscribed\r\n# no skin contact\r\n");
        let (rx, finish) = run(&board, Duration::from_millis(30));
        let seen = collect(&rx, |s| statuses(s).len() == 2);
        std::thread::sleep(Duration::from_millis(200));
        finish();
        assert!(board.heard().is_empty(), "no Ctrl-D for a running firmware");
        assert_eq!(
            rx.try_iter().count(),
            0,
            "the status stays on the last note"
        );
        assert_eq!(
            statuses(&seen),
            ["Pico connected to the strap", "Pico: no skin contact"]
        );
    }

    #[test]
    fn a_board_back_at_the_repl_is_nudged_again() {
        let board = Board::default();
        board.says(b"\n# subscribed\r\n");
        board
            .says(b"Traceback\r\nMicroPython v1.28.0\r\nType \"help()\" for more information.\r\n");
        let (rx, finish) = run(&board, Duration::from_millis(30));
        let _ = collect(&rx, |s| statuses(s).contains(&SILENT));
        finish();
        assert_eq!(board.heard(), b"\x04");
    }

    #[test]
    fn the_backlog_read_right_after_opening_is_ignored() {
        let board = Board::default();
        board.says(b"\n# no strap found# no strap found, rescanning\r\n99\r\n");
        let (rx, finish) = run_with(
            &board,
            Timing {
                quiet: Duration::from_secs(60),
                backlog: Duration::from_millis(100),
            },
        );
        std::thread::sleep(Duration::from_millis(150));
        board.says(b"7");
        board.says(b"2\r\n# subscribed\r\n73\r\n");
        let seen = collect(&rx, |s| s.iter().any(|e| matches!(e, Seen::Reading(_))));
        finish();
        assert_eq!(statuses(&seen), ["Pico connected to the strap"]);
        let readings: Vec<u16> = seen
            .iter()
            .filter_map(|s| match s {
                Seen::Reading(bpm) => Some(*bpm),
                Seen::Status(_) => None,
            })
            .collect();
        assert_eq!(
            readings,
            [73],
            "the stale 99 and the partial 72 are dropped"
        );
    }
}
