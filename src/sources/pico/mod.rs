mod firmware;
mod lines;

use std::io::{ErrorKind, Read, Write};
use std::time::{Duration, Instant};

use serialport::{SerialPort, SerialPortInfo, SerialPortType};

use super::Context;
use lines::{PicoLine, note_text, parse_pico_line};

pub use firmware::install;

pub const RP2_VID: u16 = 0x2E8A;

pub const MICROPYTHON_PID: u16 = 0x0005;

const BAUD: u32 = 115_200;

const RETRY: Duration = Duration::from_secs(3);

const QUIET: Duration = Duration::from_secs(12);

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

pub fn session(ctx: &Context, port: &str) {
    let mut serial = match open(port) {
        Ok(serial) => serial,
        Err(error) => {
            let _ = crate::log::write_changed(&format!("pico open {port}: {error}"));
            ctx.status(format!("{port} is in use by another program"));
            ctx.sleep(RETRY);
            return;
        }
    };
    let _ = crate::log::write_changed(&format!("pico opened {port}"));
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
                    match parse_pico_line(&line) {
                        PicoLine::Reading(bpm) => ctx.reading(bpm),
                        PicoLine::Note(note) => {
                            let _ = crate::log::write_changed(&format!("pico: {note}"));
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
}
