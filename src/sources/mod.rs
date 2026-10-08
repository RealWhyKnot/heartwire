use std::net::{SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::config::{Config, Service};
use crate::engine::Event;

mod auto;
pub mod ble;
pub mod http;
pub mod pico;
pub mod pulsoid;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    pub name: String,
    pub address: String,
}

pub enum Wake {
    None,
    Connect(SocketAddr),
    Shutdown(TcpStream),
}

struct Shared {
    stopped: AtomicBool,
    lock: Mutex<Wake>,
    signal: Condvar,
}

#[derive(Clone)]
pub struct Context {
    generation: u64,
    tx: Sender<Event>,
    shared: Arc<Shared>,
}

impl Context {
    fn new(generation: u64, tx: Sender<Event>) -> Self {
        Context {
            generation,
            tx,
            shared: Arc::new(Shared {
                stopped: AtomicBool::new(false),
                lock: Mutex::new(Wake::None),
                signal: Condvar::new(),
            }),
        }
    }

    pub fn reading(&self, bpm: u16) {
        let _ = self.tx.send(Event::Reading {
            generation: self.generation,
            bpm,
        });
    }

    pub fn status(&self, text: impl Into<String>) {
        let _ = self.tx.send(Event::Status {
            generation: self.generation,
            text: text.into(),
        });
    }

    pub fn devices(&self, list: Vec<Device>) {
        let _ = self.tx.send(Event::Devices {
            generation: self.generation,
            list,
        });
    }

    pub fn stopped(&self) -> bool {
        self.shared.stopped.load(Ordering::Acquire)
    }

    pub fn sleep(&self, duration: Duration) -> bool {
        let guard = self.shared.lock.lock().unwrap_or_else(|e| e.into_inner());
        let _ = self
            .shared
            .signal
            .wait_timeout_while(guard, duration, |_| !self.stopped());
        self.stopped()
    }

    pub fn set_wake(&self, wake: Wake) {
        let mut slot = self.shared.lock.lock().unwrap_or_else(|e| e.into_inner());
        *slot = wake;
        if self.stopped() {
            fire(&mut slot);
        }
    }

    fn stopper(&self) -> Stopper {
        Stopper {
            shared: self.shared.clone(),
        }
    }

    #[cfg(test)]
    pub fn test() -> (Context, std::sync::mpsc::Receiver<Event>, Stopper) {
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = Context::new(1, tx);
        let stopper = ctx.stopper();
        (ctx, rx, stopper)
    }
}

fn fire(wake: &mut Wake) {
    match std::mem::replace(wake, Wake::None) {
        Wake::None => {}
        Wake::Connect(addr) => {
            let _ = TcpStream::connect_timeout(&addr, Duration::from_millis(500));
        }
        Wake::Shutdown(stream) => {
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
    }
}

pub struct Stopper {
    shared: Arc<Shared>,
}

impl Stopper {
    pub fn stop(&self) {
        self.shared.stopped.store(true, Ordering::Release);
        let mut slot = self.shared.lock.lock().unwrap_or_else(|e| e.into_inner());
        fire(&mut slot);
        self.shared.signal.notify_all();
    }
}

pub struct Handle {
    stopper: Stopper,
    thread: Option<JoinHandle<()>>,
}

impl Handle {
    pub fn stop(mut self) {
        self.stopper.stop();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

pub fn spawn(config: &Config, generation: u64, tx: Sender<Event>) -> Handle {
    let ctx = Context::new(generation, tx);
    let stopper = ctx.stopper();
    let service = config.service_type;
    let port = config.http_server_port;
    let widget = config.stromno_widget_id.trim().to_owned();
    let device = config.bluetooth_device.trim().to_owned();
    let thread = std::thread::Builder::new()
        .name(format!("source-{}", service.key()))
        .stack_size(256 * 1024)
        .spawn(move || match service {
            Service::Auto => auto::run(&ctx, &device),
            Service::Bluetooth => {
                if ble::run(&ctx, &device, &mut || false) == ble::Outcome::NoAdapter {
                    ctx.status("No Bluetooth adapter found");
                }
            }
            Service::Pico => pico::run(&ctx),
            Service::Http => http::run(ctx, port),
            Service::Pulsoid => pulsoid::run(&ctx, &widget),
        })
        .ok();
    Handle { stopper, thread }
}
