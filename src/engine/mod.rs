#[cfg(test)]
mod tests;
mod view;

use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use crate::config::{Config, Service};
use crate::osc::{self, Arg};
use crate::sources;

pub use view::{Event, Ui, View, percent};

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
        let changed = !self.view.connected || self.view.bpm != bpm || self.view.percent != percent;
        self.set_connected(true);
        self.view.bpm = bpm;
        self.view.percent = percent;
        if changed {
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
        self.view.devices = Default::default();
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
                if *engine.view.devices != *list {
                    engine.view.devices = list.into();
                    engine.ui.show(&engine.view);
                }
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
