use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use crate::config::{Config, Service};
use crate::osc::{self, Arg};
use crate::sources::{self, Device};

pub enum Event {
    Reading { generation: u64, bpm: u16 },
    Status { generation: u64, text: String },
    Devices { generation: u64, list: Vec<Device> },
    Config(Box<Config>),
    Restart,
    InstallFirmware,
    Quit,
}

impl Event {
    #[cfg(test)]
    pub fn reading(&self) -> Option<u16> {
        match self {
            Event::Reading { bpm, .. } => Some(*bpm),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct View {
    pub connected: bool,
    pub bpm: u16,
    pub percent: f32,
    pub status: String,
    pub devices: Vec<Device>,
    pub notice: String,
}

pub trait Ui {
    fn show(&self, view: &View);
}

pub fn percent(bpm: u16, max: u32) -> f32 {
    (f32::from(bpm) / max.max(1) as f32).clamp(0.0, 1.0)
}

struct Engine<U: Ui> {
    config: Config,
    osc: osc::Sender,
    tx: Sender<Event>,
    ui: U,
    view: View,
    generation: u64,
    source: Option<sources::Handle>,
    last: Instant,
}

impl<U: Ui> Engine<U> {
    fn start_source(&mut self) {
        self.generation += 1;
        self.source = Some(sources::spawn(
            &self.config,
            self.generation,
            self.tx.clone(),
        ));
    }

    fn stop_source(&mut self) {
        if let Some(source) = self.source.take() {
            source.stop();
        }
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(u64::from(self.config.connected_timeout.max(1)))
    }

    fn set_connected(&mut self, connected: bool) {
        self.osc
            .send(&self.config.osc_path_connected, Arg::Bool(connected));
        self.view.connected = connected;
    }

    fn reading(&mut self, bpm: u16) {
        self.last = Instant::now();
        let percent = percent(bpm, self.config.max_heart_rate);
        self.osc
            .send(&self.config.osc_path_percent, Arg::Float(percent));
        let was = self.view.clone();
        self.set_connected(true);
        self.view.bpm = bpm;
        self.view.percent = percent;
        if self.view != was {
            self.ui.show(&self.view);
        }
    }

    fn apply(&mut self, config: Config) {
        let old = std::mem::replace(&mut self.config, config);
        if (old.osc_client_host.trim(), old.osc_client_port)
            != (
                self.config.osc_client_host.trim(),
                self.config.osc_client_port,
            )
        {
            self.osc.retarget(osc::resolve(
                &self.config.osc_client_host,
                self.config.osc_client_port,
            ));
        }
        let mut changed = false;
        if old.source_key() != self.config.source_key() {
            self.restart();
            changed = true;
        }
        let percent = percent(self.view.bpm, self.config.max_heart_rate);
        if percent != self.view.percent {
            self.view.percent = percent;
            changed = true;
        }
        if changed {
            self.ui.show(&self.view);
        }
    }

    fn restart(&mut self) {
        self.stop_source();
        if self.view.connected {
            self.set_connected(false);
        }
        self.view.status.clear();
        self.view.devices.clear();
        self.view.notice.clear();
        self.start_source();
    }

    fn install_firmware(&mut self) {
        let uses_pico = matches!(self.config.service_type, Service::Auto | Service::Pico);
        self.stop_source();
        let notice = match sources::pico::find_port() {
            None => "No Pico found. Plug it in with MicroPython on it.".to_owned(),
            Some(port) => {
                self.view.notice = format!("Installing on {port}...");
                self.ui.show(&self.view);
                match sources::pico::install(&port, &self.config.pico_strap_name) {
                    Ok(()) => {
                        crate::log::write(&format!("firmware installed on {port}"));
                        format!("Installed on {port}")
                    }
                    Err(error) => {
                        crate::log::write(&format!("firmware install on {port}: {error}"));
                        format!("Install failed: {error}")
                    }
                }
            }
        };
        self.view.notice = notice;
        if uses_pico {
            std::thread::sleep(Duration::from_millis(1500));
        }
        self.start_source();
        self.ui.show(&self.view);
    }
}

pub fn run(config: Config, rx: Receiver<Event>, tx: Sender<Event>, ui: impl Ui) {
    let target = osc::resolve(&config.osc_client_host, config.osc_client_port);
    let mut engine = Engine {
        config,
        osc: osc::Sender::new(target),
        tx,
        ui,
        view: View::default(),
        generation: 0,
        source: None,
        last: Instant::now(),
    };
    engine.start_source();
    engine.ui.show(&engine.view);
    loop {
        let wait = if engine.view.connected {
            (engine.last + engine.timeout()).saturating_duration_since(Instant::now())
        } else {
            Duration::from_secs(3600)
        };
        match rx.recv_timeout(wait) {
            Ok(Event::Reading { generation, bpm }) if generation == engine.generation => {
                engine.reading(bpm);
            }
            Ok(Event::Status { generation, text }) if generation == engine.generation => {
                if engine.view.status != text {
                    engine.view.status = text;
                    engine.ui.show(&engine.view);
                }
            }
            Ok(Event::Devices { generation, list }) if generation == engine.generation => {
                engine.view.devices = list;
                engine.ui.show(&engine.view);
            }
            Ok(Event::Config(config)) => engine.apply(*config),
            Ok(Event::Restart) => {
                engine.restart();
                engine.ui.show(&engine.view);
            }
            Ok(Event::InstallFirmware) => engine.install_firmware(),
            Ok(Event::Quit) | Err(RecvTimeoutError::Disconnected) => break,
            Ok(_) => {}
            Err(RecvTimeoutError::Timeout) => {
                if engine.view.connected && engine.last.elapsed() >= engine.timeout() {
                    engine.set_connected(false);
                    engine.ui.show(&engine.view);
                }
            }
        }
    }
    engine.stop_source();
    if engine.view.connected {
        engine.set_connected(false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::UdpSocket;
    use std::sync::{Arc, Mutex, mpsc};

    #[derive(Clone, Default)]
    struct Recorder(Arc<Mutex<Vec<View>>>);

    impl Ui for Recorder {
        fn show(&self, view: &View) {
            self.0.lock().unwrap().push(view.clone());
        }
    }

    fn decode(packet: &[u8]) -> (String, Arg) {
        let end = packet.iter().position(|&b| b == 0).unwrap();
        let address = String::from_utf8(packet[..end].to_vec()).unwrap();
        let tag_at = (end / 4 + 1) * 4;
        let arg = match &packet[tag_at..tag_at + 2] {
            b",f" => Arg::Float(f32::from_be_bytes(
                packet[tag_at + 4..tag_at + 8].try_into().unwrap(),
            )),
            b",T" => Arg::Bool(true),
            b",F" => Arg::Bool(false),
            other => panic!("unexpected tag {other:?}"),
        };
        (address, arg)
    }

    #[test]
    fn percent_is_clamped() {
        assert_eq!(percent(100, 200), 0.5);
        assert_eq!(percent(250, 200), 1.0);
        assert_eq!(percent(70, 0), 1.0);
    }

    #[test]
    fn readings_reach_osc_and_time_out() {
        let vrchat = UdpSocket::bind("127.0.0.1:0").unwrap();
        vrchat
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let config = Config {
            service_type: Service::Http,
            http_server_port: 0,
            osc_client_port: vrchat.local_addr().unwrap().port(),
            connected_timeout: 1,
            max_heart_rate: 200,
            ..Config::default()
        };
        let (tx, rx) = mpsc::channel();
        let ui = Recorder::default();
        let seen = ui.clone();
        let engine_tx = tx.clone();
        let worker = std::thread::spawn(move || run(config, rx, engine_tx, ui));
        tx.send(Event::Reading {
            generation: 1,
            bpm: 100,
        })
        .unwrap();
        tx.send(Event::Reading {
            generation: 99,
            bpm: 55,
        })
        .unwrap();
        let mut buf = [0u8; 128];
        let mut packets = Vec::new();
        for _ in 0..3 {
            let n = vrchat.recv(&mut buf).unwrap();
            packets.push(decode(&buf[..n]));
        }
        assert_eq!(
            packets[0],
            ("/avatar/parameters/hr_percent".into(), Arg::Float(0.5))
        );
        assert_eq!(
            packets[1],
            ("/avatar/parameters/hr_connected".into(), Arg::Bool(true))
        );
        assert_eq!(
            packets[2],
            ("/avatar/parameters/hr_connected".into(), Arg::Bool(false))
        );
        tx.send(Event::Quit).unwrap();
        worker.join().unwrap();
        let views = seen.0.lock().unwrap();
        assert!(
            views
                .iter()
                .any(|v| v.connected && v.bpm == 100 && v.percent == 0.5)
        );
        assert!(!views.last().unwrap().connected);
        assert!(views.iter().all(|v| v.bpm != 55));
    }
}
