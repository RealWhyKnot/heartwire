use std::time::{Duration, Instant};

use super::{Context, ble, pico};

pub fn run(ctx: &Context, device: &str) {
    let mut bluetooth_retry = Instant::now();
    while !ctx.stopped() {
        if let Some(port) = pico::find_port() {
            pico::session(ctx, &port);
            continue;
        }
        if Instant::now() >= bluetooth_retry {
            let mut ticks = 0u32;
            let outcome = ble::run(ctx, device, &mut || {
                ticks += 1;
                ticks.is_multiple_of(5) && pico::find_port().is_some()
            });
            if outcome == ble::Outcome::NoAdapter {
                bluetooth_retry = Instant::now() + Duration::from_secs(60);
            }
            continue;
        }
        ctx.status("Waiting for a Pico or a Bluetooth adapter");
        ctx.sleep(Duration::from_secs(3));
    }
}
