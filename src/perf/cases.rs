use std::hint::black_box;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;
use std::time::Duration;

use sha2::{Digest, Sha256};

use super::Case;
use crate::config::{Config, Service, Store};
use crate::engine::{self, Event, Ui, View};
use crate::osc::{self, Arg};
use crate::sources::{self, Context, Device, Stopper};
use crate::update::{self, AppVersion, Release};
use crate::version::Channel;

const NS: Duration = Duration::from_nanos(1);
const US: Duration = Duration::from_micros(1);
const MS: Duration = Duration::from_millis(1);

const HR_OSC_CONFIG: &str = r#"{"service_type":"http","http_server_port":8080,"stromno_widget_id":"3f2b8c1e-5d4a-4e6f-9a7b-2c1d0e9f8a7b","osc_path_connected":"/avatar/parameters/hr_connected","osc_path_percent":"/avatar/parameters/hr_percent","connected_timeout":60,"max_heart_rate":230,"osc_client_host":"127.0.0.1","osc_client_port":9000}"#;

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("heartwire-perf-{name}-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

struct Signal(Sender<u16>);

impl Ui for Signal {
    fn show(&self, view: &View) {
        let _ = self.0.send(view.bpm);
    }
}

struct Pipeline {
    tx: Sender<Event>,
    shown: Receiver<u16>,
    worker: Option<JoinHandle<()>>,
    bpm: u16,
    _osc: UdpSocket,
}

impl Pipeline {
    fn start() -> Pipeline {
        let osc = UdpSocket::bind("127.0.0.1:0").unwrap();
        let config = Config {
            service_type: Service::Http,
            http_server_port: 0,
            osc_client_port: osc.local_addr().unwrap().port(),
            ..Config::default()
        };
        let (tx, rx) = channel();
        let (seen, shown) = channel();
        let engine_tx = tx.clone();
        let worker = std::thread::spawn(move || engine::run(config, rx, engine_tx, Signal(seen)));
        let _ = shown.recv();
        Pipeline {
            tx,
            shown,
            worker: Some(worker),
            bpm: 60,
            _osc: osc,
        }
    }

    fn reading(&mut self) {
        self.bpm = if self.bpm == 60 { 61 } else { 60 };
        let _ = self.tx.send(Event::Reading {
            generation: 1,
            bpm: self.bpm,
        });
        while self.shown.recv().is_ok_and(|bpm| bpm != self.bpm) {}
    }
}

impl Drop for Pipeline {
    fn drop(&mut self) {
        let _ = self.tx.send(Event::Quit);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

struct HttpServer {
    port: u16,
    rx: Receiver<Event>,
    stop: Stopper,
    worker: Option<JoinHandle<()>>,
}

impl HttpServer {
    fn start() -> HttpServer {
        let (ctx, rx, stop) = Context::standalone();
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let worker = std::thread::spawn(move || sources::http::run(ctx, port));
        while TcpStream::connect(("127.0.0.1", port)).is_err() {
            std::thread::sleep(MS);
        }
        HttpServer {
            port,
            rx,
            stop,
            worker: Some(worker),
        }
    }

    fn post(&mut self) {
        if let Ok(mut stream) = TcpStream::connect(("127.0.0.1", self.port)) {
            let _ = stream.write_all(b"POST / HTTP/1.1\r\nContent-Length: 2\r\n\r\n72");
            let mut reply = Vec::new();
            let _ = stream.read_to_end(&mut reply);
            black_box(reply);
        }
        while self.rx.try_recv().is_ok() {}
    }
}

impl Drop for HttpServer {
    fn drop(&mut self) {
        self.stop.stop();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn releases() -> Vec<Release> {
    (1..=20)
        .map(|day| Release {
            tag_name: format!(
                "v2026.10.{day}.0{}",
                if day % 3 == 0 { "-beta" } else { "" }
            ),
            draft: false,
            prerelease: day % 3 == 0,
            assets: Vec::new(),
        })
        .collect()
}

fn devices() -> Vec<Device> {
    (0..12)
        .map(|i| Device {
            name: format!("Strap {i}"),
            address: format!("C0:FF:EE:00:00:{i:02X}"),
        })
        .collect()
}

pub fn all() -> Vec<Case> {
    #[cfg_attr(not(windows), allow(unused_mut))]
    let mut cases = vec![
        Case::new("core", "parse a BLE heart rate packet", 100 * NS, || {
            black_box(
                crate::heart_rate::parse_measurement(black_box(&[0x16, 72, 0x10, 0x03]))
                    .and_then(crate::heart_rate::usable),
            );
        }),
        Case::new("core", "parse a bpm from text", 500 * NS, || {
            black_box(crate::heart_rate::parse_bpm_text(black_box(" 71.6\n")));
        }),
        Case::new("core", "percent of the maximum", 50 * NS, || {
            black_box(engine::percent(black_box(131), black_box(230)));
        }),
        Case::new("osc", "encode a float message", 300 * NS, {
            let mut buf = Vec::with_capacity(64);
            move || {
                osc::encode(
                    black_box("/avatar/parameters/hr_percent"),
                    Arg::Float(0.5),
                    &mut buf,
                );
                black_box(&buf);
            }
        }),
        Case::new("osc", "send a message over UDP", 50 * US, {
            let receiver = UdpSocket::bind("127.0.0.1:0").unwrap();
            let mut sender = osc::Sender::new(receiver.local_addr().unwrap());
            move || {
                sender.send("/avatar/parameters/hr_percent", Arg::Float(0.5));
                black_box(&receiver);
            }
        }),
        Case::new("engine", "reading to OSC and the window", 200 * US, {
            let mut pipeline = Pipeline::start();
            move || pipeline.reading()
        }),
        Case::new("sources", "parse an HTTP POST", 2 * US, || {
            black_box(sources::http::parse_request(black_box(
                b"POST / HTTP/1.1\r\nHost: localhost:8080\r\nContent-Length: 2\r\nContent-Type: application/x-www-form-urlencoded\r\n\r\n60",
            )));
        }),
        Case::new("sources", "parse a chunked HTTP POST", 2 * US, || {
            black_box(sources::http::parse_request(black_box(
                b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n1\r\n7\r\n1\r\n2\r\n0\r\n\r\n",
            )));
        }),
        Case::new("sources", "HTTP reading over loopback", 5 * MS, {
            let mut server = HttpServer::start();
            move || server.post()
        }),
        Case::new("sources", "parse a Pico line", 100 * NS, || {
            black_box(sources::pico::parse_pico_line(black_box(b"72\r")));
        }),
        Case::new(
            "sources",
            "split 64 Pico lines from serial bytes",
            20 * US,
            {
                let bytes: Vec<u8> = (0..64)
                    .flat_map(|i| format!("{}\r\n", 60 + i % 40).into_bytes())
                    .collect();
                move || {
                    let mut lines = sources::pico::LineBuffer::default();
                    let mut readings = 0;
                    lines.feed(b"\n", |_| {});
                    lines.feed(black_box(&bytes), |line| {
                        if matches!(
                            sources::pico::parse_pico_line(line),
                            sources::pico::PicoLine::Reading(_)
                        ) {
                            readings += 1;
                        }
                    });
                    black_box(readings);
                }
            },
        ),
        Case::new("sources", "Pico note to status text", 500 * NS, || {
            black_box(sources::pico::note_text(black_box("found COOSPO HW807")));
        }),
        Case::new("sources", "parse a Pulsoid message", 5 * US, || {
            black_box(sources::pulsoid::bpm_from_message(black_box(
                r#"{"timestamp":1760054400000,"data":{"heartRate":72}}"#,
            )));
        }),
        Case::new("sources", "drop a repeated status line", 500 * NS, {
            let (ctx, rx, _stop) = Context::standalone();
            move || {
                ctx.status("Pico: COOSPO HW807");
                black_box(&rx);
            }
        }),
        Case::new("firmware", "build the firmware upload script", MS, || {
            black_box(sources::pico::write_file_script(
                "main.py",
                sources::pico::FIRMWARE_MAIN,
            ));
        }),
        Case::new("config", "read hr-osc settings", 100 * US, || {
            black_box(Config::from_json(black_box(HR_OSC_CONFIG)));
        }),
        Case::new("config", "write settings JSON", 20 * US, || {
            black_box(serde_json::to_string_pretty(&Config::default()).ok());
        }),
        Case::new("config", "save and load the settings file", 20 * MS, {
            let store = Store::new(&scratch("store"));
            let config = Config::default();
            move || {
                store.save(&config);
                black_box(store.load());
            }
        }),
        Case::new("update", "parse a version tag", 500 * NS, || {
            black_box(AppVersion::parse(black_box("v2026.10.9.0-beta")));
        }),
        Case::new("update", "pick the newest of 20 releases", 10 * US, {
            let list = releases();
            let current = AppVersion::parse("v2026.10.9.0").unwrap();
            move || {
                black_box(update::select(&list, current, Channel::Beta));
            }
        }),
        Case::new("update", "parse an integrity row", 5 * US, {
            let row = format!("{}\t1234\theartwire.zip\n", "a".repeat(64));
            move || {
                black_box(update::parse_integrity(&row, "heartwire.zip").ok());
            }
        }),
        Case::new("update", "SHA-256 of 1 MiB", 20 * MS, {
            let data = vec![0x5au8; 1 << 20];
            move || {
                black_box(Sha256::digest(&data));
            }
        }),
        Case::new("app", "build 12 device rows", 50 * US, {
            let list = devices();
            move || {
                black_box(crate::app::devices::rows(&list, "C0:FF:EE:00:00:05"));
            }
        }),
        Case::new("log", "format a log timestamp", 2 * US, || {
            black_box(crate::log::timestamp(black_box(1_791_504_000)));
        }),
        Case::new("platform", "check the serial port list", 50 * US, || {
            black_box(crate::platform::serial_ports_key());
        }),
        Case::new("platform", "enumerate serial ports", 15 * MS, || {
            black_box(sources::pico::find_port());
        }),
        Case::new("platform", "read the autostart entry", MS, || {
            black_box(crate::platform::autostart_enabled());
        }),
        Case::new("net", "build an HTTPS agent", 20 * MS, || {
            black_box(crate::net::agent(Duration::from_secs(5)));
        }),
    ];
    #[cfg(windows)]
    cases.push(Case::new(
        "platform",
        "check whether SteamVR runs",
        20 * MS,
        || {
            black_box(crate::platform::process_running("vrserver.exe"));
        },
    ));
    cases
}
