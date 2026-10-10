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

fn device(name: &str) -> crate::sources::Device {
    crate::sources::Device {
        name: name.into(),
        address: format!("C0:FF:EE:00:00:{:02X}", name.len()),
    }
}

struct Running {
    tx: mpsc::Sender<Event>,
    seen: Recorder,
    worker: std::thread::JoinHandle<()>,
}

impl Running {
    fn start(config: Config) -> Running {
        let (tx, rx) = mpsc::channel();
        let seen = Recorder::default();
        let ui = seen.clone();
        let engine_tx = tx.clone();
        let worker = std::thread::spawn(move || run(config, rx, engine_tx, ui));
        Running { tx, seen, worker }
    }

    fn send(&self, event: Event) {
        self.tx.send(event).unwrap();
    }

    fn finish(self) -> Vec<View> {
        self.tx.send(Event::Quit).unwrap();
        self.worker.join().unwrap();
        self.seen.0.lock().unwrap().clone()
    }
}

fn quiet_config() -> Config {
    Config {
        service_type: Service::Http,
        http_server_port: 0,
        osc_client_port: 9,
        ..Config::default()
    }
}

fn status(generation: u64, text: &str) -> Event {
    Event::Status {
        generation,
        text: text.into(),
    }
}

#[test]
fn the_window_updates_only_when_something_changed() {
    let engine = Running::start(quiet_config());
    for _ in 0..10 {
        engine.send(Event::Reading {
            generation: 1,
            bpm: 100,
        });
    }
    engine.send(Event::Reading {
        generation: 1,
        bpm: 101,
    });
    for _ in 0..3 {
        engine.send(status(1, "Pico on COM5"));
    }
    let views = engine.finish();
    let bpms: Vec<u16> = views.iter().map(|v| v.bpm).collect();
    assert_eq!(bpms, [0, 100, 101, 101], "start, two readings, one status");
    assert_eq!(views[3].status, "Pico on COM5");
}

#[test]
fn a_device_list_is_shown_once_and_shared_not_copied() {
    let engine = Running::start(quiet_config());
    let list = vec![device("Polar H10"), device("COOSPO")];
    for _ in 0..3 {
        engine.send(Event::Devices {
            generation: 1,
            list: list.clone(),
        });
    }
    let views = engine.finish();
    assert_eq!(views.len(), 2);
    assert_eq!(&*views[1].devices, list.as_slice());
}

#[test]
fn events_from_an_old_source_are_dropped() {
    let engine = Running::start(quiet_config());
    engine.send(status(7, "stale"));
    engine.send(Event::Devices {
        generation: 7,
        list: vec![device("old")],
    });
    engine.send(Event::Reading {
        generation: 7,
        bpm: 90,
    });
    engine.send(status(1, "current"));
    let views = engine.finish();
    assert!(
        views
            .iter()
            .all(|v| v.status != "stale" && v.devices.is_empty() && v.bpm == 0)
    );
    assert_eq!(views.last().unwrap().status, "current");
}

#[test]
fn only_source_settings_restart_the_source() {
    let engine = Running::start(quiet_config());
    engine.send(Event::Reading {
        generation: 1,
        bpm: 100,
    });
    engine.send(Event::Config(Box::new(Config {
        max_heart_rate: 100,
        ..quiet_config()
    })));
    engine.send(status(1, "same source"));
    engine.send(Event::Config(Box::new(Config {
        max_heart_rate: 100,
        stromno_widget_id: "abc".into(),
        ..quiet_config()
    })));
    engine.send(status(1, "old source"));
    engine.send(status(2, "new source"));
    let views = engine.finish();
    assert!(
        views.iter().any(|v| v.percent == 1.0),
        "max_heart_rate applied"
    );
    assert!(views.iter().any(|v| v.status == "same source"));
    assert!(views.iter().all(|v| v.status != "old source"));
    let last = views.last().unwrap();
    assert_eq!(last.status, "new source");
    assert!(!last.connected, "a restart drops the connection");
}

#[test]
fn restart_clears_what_the_old_source_reported() {
    let engine = Running::start(quiet_config());
    engine.send(status(1, "Pico on COM5"));
    engine.send(Event::Devices {
        generation: 1,
        list: vec![device("Polar")],
    });
    engine.send(Event::Restart);
    let views = engine.finish();
    let last = views.last().unwrap();
    assert!(last.status.is_empty() && last.devices.is_empty() && last.notice.is_empty());
}

#[test]
fn osc_follows_a_new_target() {
    let first = UdpSocket::bind("127.0.0.1:0").unwrap();
    let second = UdpSocket::bind("127.0.0.1:0").unwrap();
    for socket in [&first, &second] {
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
    }
    let config = |port| Config {
        osc_client_port: port,
        ..quiet_config()
    };
    let engine = Running::start(config(first.local_addr().unwrap().port()));
    engine.send(Event::Reading {
        generation: 1,
        bpm: 100,
    });
    let mut buf = [0u8; 128];
    let n = first.recv(&mut buf).unwrap();
    assert_eq!(decode(&buf[..n]).1, Arg::Float(0.5));
    engine.send(Event::Config(Box::new(config(
        second.local_addr().unwrap().port(),
    ))));
    engine.send(Event::Reading {
        generation: 1,
        bpm: 50,
    });
    let n = second.recv(&mut buf).unwrap();
    assert_eq!(decode(&buf[..n]).1, Arg::Float(0.25));
    engine.finish();
}

#[test]
fn a_post_to_the_http_source_reaches_vrchat_then_times_out() {
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};

    let vrchat = UdpSocket::bind("127.0.0.1:0").unwrap();
    vrchat
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let engine = Running::start(Config {
        service_type: Service::Http,
        http_server_port: port,
        osc_client_port: vrchat.local_addr().unwrap().port(),
        connected_timeout: 1,
        max_heart_rate: 200,
        ..Config::default()
    });
    let mut reply = String::new();
    for _ in 0..100 {
        if let Ok(mut stream) = TcpStream::connect(("127.0.0.1", port)) {
            stream
                .write_all(b"POST / HTTP/1.1\r\nContent-Length: 3\r\n\r\n150")
                .unwrap();
            stream.read_to_string(&mut reply).unwrap();
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(reply.starts_with("HTTP/1.1 200 OK"), "{reply}");
    let mut buf = [0u8; 128];
    let mut packets = Vec::new();
    for _ in 0..3 {
        let n = vrchat.recv(&mut buf).unwrap();
        packets.push(decode(&buf[..n]));
    }
    assert_eq!(
        packets,
        [
            ("/avatar/parameters/hr_percent".into(), Arg::Float(0.75)),
            ("/avatar/parameters/hr_connected".into(), Arg::Bool(true)),
            ("/avatar/parameters/hr_connected".into(), Arg::Bool(false)),
        ]
    );
    let views = engine.finish();
    assert!(views.iter().any(|v| v.connected && v.bpm == 150));
    assert!(!views.last().unwrap().connected);
}
