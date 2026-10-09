use std::io::{ErrorKind, Write};
use std::time::{Duration, Instant};

use serialport::SerialPort;

use super::open;

pub const FIRMWARE_MAIN: &str = include_str!("../../../firmware/main.py");

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
