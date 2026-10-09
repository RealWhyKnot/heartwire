use std::sync::mpsc::Sender;

use crate::config::{Config, Service};
use crate::engine::Event;

mod auto;
pub mod ble;
mod context;
pub mod http;
pub mod pico;
pub mod pulsoid;

pub use context::{Context, Handle, Stopper, Wake};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    pub name: String,
    pub address: String,
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
