use std::fmt::Write as _;
use std::io::{ErrorKind, Read, Write};
use std::time::{Duration, Instant};

use super::open;

pub const FIRMWARE_MAIN: &str = include_str!("../../../firmware/main.py");

const RAW_PROMPT: &[u8] = b"raw REPL; CTRL-B to exit\r\n>";

pub fn install(port: &str, strap_name: &str) -> Result<(), String> {
    let mut serial = open(port).map_err(|e| format!("{port}: {e}"))?;
    serial
        .set_timeout(Duration::from_millis(200))
        .map_err(|e| e.to_string())?;
    upload(&mut *serial, strap_name)
}

pub fn strap_config(strap_name: &str) -> String {
    format!(
        "# Matched case-insensitively against the advertised name; empty means the first strap found.\nDEVICE_NAME = {}\n",
        python_string(strap_name.trim())
    )
}

fn upload<P: Read + Write + ?Sized>(board: &mut P, strap_name: &str) -> Result<(), String> {
    let io = |e: std::io::Error| e.to_string();
    board.write_all(b"\r\x03\x03").map_err(io)?;
    drain(board, Duration::from_millis(400));
    board.write_all(b"\r\x01").map_err(io)?;
    read_until(board, RAW_PROMPT, Duration::from_secs(3))?;
    exec(board, &write_file_script("main.py", FIRMWARE_MAIN))?;
    exec(
        board,
        &write_file_script("hr_config.py", &strap_config(strap_name)),
    )?;
    board.write_all(b"\x02").map_err(io)?;
    drain(board, Duration::from_millis(200));
    board.write_all(b"\x04").map_err(io)?;
    board.flush().map_err(io)?;
    Ok(())
}

pub fn write_file_script(name: &str, content: &str) -> String {
    let mut script = format!("f=open({},'wb')\nw=f.write\n", python_string(name));
    for chunk in content.as_bytes().chunks(192) {
        script.push_str("w(b'");
        for &b in chunk {
            match b {
                b'\\' => script.push_str("\\\\"),
                b'\'' => script.push_str("\\'"),
                0x20..=0x7e => script.push(b as char),
                _ => {
                    let _ = write!(script, "\\x{b:02x}");
                }
            }
        }
        script.push_str("')\n");
    }
    script.push_str("f.close()\n");
    script
}

pub fn python_string(text: &str) -> String {
    let mut out = String::from("'");
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            ' '..='~' => out.push(c),
            _ if u32::from(c) > 0xffff => {
                let _ = write!(out, "\\U{:08x}", u32::from(c));
            }
            _ => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
        }
    }
    out.push('\'');
    out
}

fn exec<P: Read + Write + ?Sized>(board: &mut P, script: &str) -> Result<(), String> {
    for chunk in script.as_bytes().chunks(256) {
        board.write_all(chunk).map_err(|e| e.to_string())?;
        std::thread::sleep(Duration::from_millis(10));
    }
    board.write_all(b"\x04").map_err(|e| e.to_string())?;
    let reply = read_until(board, b"\x04>", Duration::from_secs(10))?;
    raw_reply_error(&reply)
}

pub fn raw_reply_error(reply: &[u8]) -> Result<(), String> {
    let start = reply
        .windows(2)
        .position(|w| w == b"OK")
        .ok_or("the board rejected the upload")?;
    let error = reply[start + 2..]
        .split(|&b| b == 0x04)
        .nth(1)
        .unwrap_or_default();
    if error.iter().any(|b| !b.is_ascii_whitespace()) {
        return Err(String::from_utf8_lossy(error).trim().to_owned());
    }
    Ok(())
}

fn read_until<P: Read + ?Sized>(
    board: &mut P,
    marker: &[u8],
    limit: Duration,
) -> Result<Vec<u8>, String> {
    let end = Instant::now() + limit;
    let mut seen = Vec::new();
    let mut buf = [0u8; 256];
    while Instant::now() < end {
        match board.read(&mut buf) {
            Ok(n) => {
                seen.extend_from_slice(&buf[..n]);
                if seen.windows(marker.len()).any(|w| w == marker) {
                    return Ok(seen);
                }
            }
            Err(e) if e.kind() == ErrorKind::TimedOut => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    Err("the board did not answer; is MicroPython installed?".into())
}

fn drain<P: Read + ?Sized>(board: &mut P, limit: Duration) {
    let end = Instant::now() + limit;
    let mut buf = [0u8; 256];
    while Instant::now() < end {
        if board.read(&mut buf).is_err() {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_script_round_trips_awkward_bytes() {
        let script = write_file_script("main.py", "a='\\x'\n\u{e9}");
        assert!(script.starts_with("f=open('main.py','wb')\nw=f.write\n"));
        assert!(script.contains("w(b'a=\\'\\\\x\\'\\x0a\\xc3\\xa9')"));
        assert!(script.ends_with("f.close()\n"));
    }

    #[test]
    fn firmware_script_stays_ascii() {
        let script = write_file_script("main.py", FIRMWARE_MAIN);
        assert!(script.is_ascii());
        assert!(FIRMWARE_MAIN.contains("HR_MEASUREMENT"));
    }

    #[test]
    fn raw_repl_replies() {
        assert_eq!(raw_reply_error(b"OK\x04\x04>"), Ok(()));
        assert_eq!(raw_reply_error(b"OKhello\r\n\x04\x04>"), Ok(()));
        assert_eq!(
            raw_reply_error(b"OK\x04Traceback: OSError\r\n\x04>"),
            Err("Traceback: OSError".into())
        );
        assert!(raw_reply_error(b"\x04>").is_err());
    }

    #[test]
    fn python_strings_are_escaped() {
        assert_eq!(python_string(""), "''");
        assert_eq!(python_string("Polar H10"), "'Polar H10'");
        assert_eq!(python_string("it's"), "'it\\'s'");
    }

    #[test]
    fn characters_beyond_the_basic_plane_keep_all_their_bits() {
        let heart = char::from_u32(0x1f493).unwrap();
        let accent = char::from_u32(0xe9).unwrap();
        assert_eq!(
            python_string(&format!("Strap {heart}{accent}")),
            "'Strap \\U0001f493\\u00e9'"
        );
    }

    #[derive(Default)]
    struct RawRepl {
        raw: bool,
        pending: Vec<u8>,
        script: Vec<u8>,
        scripts: Vec<Vec<u8>>,
        resets: usize,
    }

    impl Read for RawRepl {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.pending.is_empty() {
                return Err(ErrorKind::TimedOut.into());
            }
            let n = self.pending.len().min(buf.len());
            buf[..n].copy_from_slice(&self.pending[..n]);
            self.pending.drain(..n);
            Ok(n)
        }
    }

    impl Write for RawRepl {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            match buf {
                b"\r\x01" => {
                    self.raw = true;
                    self.pending.extend_from_slice(RAW_PROMPT);
                }
                b"\x02" => self.raw = false,
                b"\x04" if self.raw => {
                    self.scripts.push(std::mem::take(&mut self.script));
                    self.pending.extend_from_slice(b"OK\x04\x04>");
                }
                b"\x04" => self.resets += 1,
                _ if self.raw => self.script.extend_from_slice(buf),
                _ => {}
            }
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn upload_writes_both_files_through_the_raw_repl_then_resets() {
        let mut board = RawRepl::default();
        upload(&mut board, " Polar H10 ").unwrap();
        assert_eq!(board.scripts.len(), 2);
        assert_eq!(
            board.scripts[0],
            write_file_script("main.py", FIRMWARE_MAIN).into_bytes()
        );
        assert_eq!(
            board.scripts[1],
            write_file_script("hr_config.py", &strap_config("Polar H10")).into_bytes()
        );
        assert!(strap_config("Polar H10").contains("DEVICE_NAME = 'Polar H10'"));
        assert!(!board.raw, "the upload leaves the raw REPL");
        assert_eq!(board.resets, 1, "the board restarts into the new firmware");
    }

    #[test]
    fn a_board_without_micropython_is_reported() {
        struct Mute;
        impl Read for Mute {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(ErrorKind::TimedOut.into())
            }
        }
        impl Write for Mute {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let error = upload(&mut Mute, "").unwrap_err();
        assert!(error.contains("is MicroPython installed"), "{error}");
    }

    #[test]
    #[ignore = "writes the firmware to the Pico plugged into this computer"]
    fn installs_on_the_connected_pico() {
        let port = super::super::find_port().expect("a Pico is plugged in");
        install(&port, "").expect("the firmware installs");
        std::thread::sleep(Duration::from_secs(2));
        let mut serial = open(&port).expect("the board comes back after the reset");
        let reply = read_until(&mut *serial, b"\n#", Duration::from_secs(25))
            .or_else(|_| read_until(&mut *serial, b"\r\n", Duration::from_secs(5)))
            .expect("the firmware prints a line");
        assert!(!reply.is_empty());
    }
}
