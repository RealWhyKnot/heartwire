mod firmware;
mod lines;
mod link;
mod watch;

use std::time::Duration;

use serialport::{SerialPort, SerialPortInfo, SerialPortType};

use super::Context;

pub use firmware::{FIRMWARE_MAIN, install, write_file_script};
pub use lines::{LineBuffer, PicoLine, note_text, parse_pico_line};
pub use watch::PortWatch;

pub const RP2_VID: u16 = 0x2E8A;

pub const MICROPYTHON_PID: u16 = 0x0005;

const BAUD: u32 = 115_200;

const RETRY: Duration = Duration::from_secs(3);

const TIMING: link::Timing = link::Timing {
    quiet: Duration::from_secs(12),
    backlog: Duration::from_millis(300),
};

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
    let mut watch = PortWatch::default();
    while !ctx.stopped() {
        match watch.find() {
            Some(port) => {
                session(ctx, &port);
                watch.forget();
            }
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
    if let Err(error) = link::pump(ctx, port, &mut *serial, TIMING) {
        crate::log::write(&format!("pico {port} lost: {error}"));
        ctx.status("Pico unplugged, waiting");
        ctx.sleep(RETRY);
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
