use std::io::{ErrorKind, Read, Write};
use std::time::{Duration, Instant};

use serialport::{SerialPort, SerialPortInfo, SerialPortType};

use super::Context;
use crate::hr::{self, PicoLine};

pub const RP2_VID: u16 = 0x2E8A;
pub const MICROPYTHON_PID: u16 = 0x0005;
const BAUD: u32 = 115_200;
const RETRY: Duration = Duration::from_secs(3);
const QUIET: Duration = Duration::from_secs(12);

pub const FIRMWARE_MAIN: &str = include_str!("../../firmware/main.py");

pub fn pick(ports: &[SerialPortInfo]) -> Option<String> {
    let usb = |p: &&SerialPortInfo| match &p.port_type {
        SerialPortType::UsbPort(info) => Some((info.vid, info.pid)),
        _ => None,
    };
    ports
        .iter()
        .find(|p| usb(p) == Some((RP2_VID, MICROPYTHON_PID)))
        .or_else(|| {
            ports
                .iter()
                .find(|p| usb(p).is_some_and(|(vid, _)| vid == RP2_VID))
        })
        .map(|p| p.port_name.clone())
}

pub fn find_port() -> Option<String> {
    pick(&serialport::available_ports().ok()?)
}

pub fn run(ctx: &Context) {
    while !ctx.stopped() {
        match find_port() {
            Some(port) => session(ctx, &port),
            None => {
                ctx.status("Waiting for a Pico");
                ctx.sleep(RETRY);
            }
        }
    }
}

fn open(port: &str) -> serialport::Result<Box<dyn SerialPort>> {
    let mut serial = serialport::new(port, BAUD)
        .timeout(Duration::from_secs(1))
        .open()?;
    serial.write_data_terminal_ready(true)?;
    Ok(serial)
}

pub fn note_text(note: &str) -> String {
    if let Some(name) = note.strip_prefix("found ") {
        return format!("Pico found {name}");
    }
    match note {
        "subscribed" => "Pico connected to the strap".into(),
        "no strap found, rescanning" => "Pico is searching for a strap".into(),
        "no skin contact" => "Pico: no skin contact".into(),
        "strap disconnected" => "Pico lost the strap".into(),
        other => format!("Pico: {other}"),
    }
}

pub fn session(ctx: &Context, port: &str) {
    let mut serial = match open(port) {
        Ok(serial) => serial,
        Err(error) => {
            crate::log::write(&format!("pico open {port}: {error}"));
            ctx.status(format!("{port} is in use by another program"));
            ctx.sleep(RETRY);
            return;
        }
    };
    crate::log::write(&format!("pico opened {port}"));
    ctx.status(format!("Pico on {port}"));
    let mut line = Vec::with_capacity(64);
    let mut buf = [0u8; 256];
    let mut heard = Instant::now();
    let mut poked = false;
    while !ctx.stopped() {
        if heard.elapsed() >= QUIET {
            if !poked {
                crate::log::write(&format!("pico {port} is quiet, asking it to restart"));
                let _ = serial.write_all(b"\x04");
                poked = true;
            } else {
                ctx.status("The Pico is silent. Install the firmware in Settings > Pico");
            }
            heard = Instant::now();
        }
        match serial.read(&mut buf) {
            Ok(0) => std::thread::sleep(Duration::from_millis(100)),
            Ok(n) => {
                if buf[..n].contains(&b'\n') {
                    heard = Instant::now();
                }
                for &byte in &buf[..n] {
                    if byte != b'\n' {
                        if line.len() < 256 {
                            line.push(byte);
                        }
                        continue;
                    }
                    match hr::parse_pico_line(&line) {
                        PicoLine::Reading(bpm) => ctx.reading(bpm),
                        PicoLine::Note(note) => {
                            crate::log::write(&format!("pico: {note}"));
                            ctx.status(note_text(note));
                        }
                        PicoLine::Other => {}
                    }
                    line.clear();
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    ErrorKind::TimedOut | ErrorKind::WouldBlock | ErrorKind::Interrupted
                ) => {}
            Err(error) => {
                crate::log::write(&format!("pico {port} lost: {error}"));
                ctx.status("Pico unplugged, waiting");
                ctx.sleep(RETRY);
                return;
            }
        }
    }
}

pub fn install(port: &str, strap_name: &str) -> Result<(), String> {
    let mut serial = open(port).map_err(|e| format!("{port}: {e}"))?;
    serial
        .set_timeout(Duration::from_millis(200))
        .map_err(|e| e.to_string())?;
    let io = |e: std::io::Error| e.to_string();
    serial.write_all(b"\r\x03\x03").map_err(io)?;
    drain(&mut *serial, Duration::from_millis(400));
    serial.write_all(b"\r\x01").map_err(io)?;
    expect(
        &mut *serial,
        b"raw REPL; CTRL-B to exit\r\n>",
        Duration::from_secs(3),
    )?;
    exec(&mut *serial, &write_file_script("main.py", FIRMWARE_MAIN))?;
    let config = format!(
        "# Matched case-insensitively against the advertised name; empty means the first strap found.\nDEVICE_NAME = {}\n",
        python_string(strap_name.trim())
    );
    exec(&mut *serial, &write_file_script("hr_config.py", &config))?;
    serial.write_all(b"\x02").map_err(io)?;
    drain(&mut *serial, Duration::from_millis(200));
    serial.write_all(b"\x04").map_err(io)?;
    serial.flush().map_err(io)?;
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
                _ => script.push_str(&format!("\\x{b:02x}")),
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
            _ => out.push_str(&format!("\\u{:04x}", c as u32 & 0xffff)),
        }
    }
    out.push('\'');
    out
}

fn exec(serial: &mut dyn SerialPort, script: &str) -> Result<(), String> {
    for chunk in script.as_bytes().chunks(256) {
        serial.write_all(chunk).map_err(|e| e.to_string())?;
        std::thread::sleep(Duration::from_millis(10));
    }
    serial.write_all(b"\x04").map_err(|e| e.to_string())?;
    let reply = read_until(serial, b"\x04>", Duration::from_secs(10))?;
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

fn expect(serial: &mut dyn SerialPort, marker: &[u8], limit: Duration) -> Result<(), String> {
    read_until(serial, marker, limit).map(|_| ())
}

fn read_until(
    serial: &mut dyn SerialPort,
    marker: &[u8],
    limit: Duration,
) -> Result<Vec<u8>, String> {
    let end = Instant::now() + limit;
    let mut seen = Vec::new();
    let mut buf = [0u8; 256];
    while Instant::now() < end {
        match serial.read(&mut buf) {
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

fn drain(serial: &mut dyn SerialPort, limit: Duration) {
    let end = Instant::now() + limit;
    let mut buf = [0u8; 256];
    while Instant::now() < end {
        if serial.read(&mut buf).is_err() {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serialport::UsbPortInfo;

    fn usb(name: &str, vid: u16, pid: u16) -> SerialPortInfo {
        SerialPortInfo {
            port_name: name.into(),
            port_type: SerialPortType::UsbPort(UsbPortInfo {
                vid,
                pid,
                serial_number: None,
                manufacturer: None,
                product: None,
            }),
        }
    }

    #[test]
    fn prefers_a_micropython_board() {
        let ports = vec![
            usb("COM3", 0x1234, 0x0001),
            usb("COM4", RP2_VID, 0x000a),
            usb("COM5", RP2_VID, MICROPYTHON_PID),
        ];
        assert_eq!(pick(&ports).as_deref(), Some("COM5"));
        assert_eq!(pick(&ports[..2]).as_deref(), Some("COM4"));
        assert_eq!(pick(&ports[..1]), None);
        assert_eq!(pick(&[]), None);
    }

    #[test]
    fn notes_read_as_status_lines() {
        assert_eq!(note_text("found COOSPO HW807"), "Pico found COOSPO HW807");
        assert_eq!(note_text("subscribed"), "Pico connected to the strap");
        assert_eq!(note_text("error: OSError(5)"), "Pico: error: OSError(5)");
    }

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
}
