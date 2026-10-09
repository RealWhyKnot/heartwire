use std::future::Future;
use std::time::{Duration, Instant};

use btleplug::api::bleuuid::uuid_from_u16;
use btleplug::api::{Central, CentralEvent, Manager as _, Peripheral as _, ScanFilter};
use btleplug::platform::{Adapter, Manager, Peripheral};
use futures::StreamExt;
use uuid::Uuid;

use super::{Context, Device};
use crate::heart_rate;

const HR_SERVICE: Uuid = uuid_from_u16(0x180D);
const HR_MEASUREMENT: Uuid = uuid_from_u16(0x2A37);
const STEP: Duration = Duration::from_secs(1);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
const SILENCE_CHECK: Duration = Duration::from_secs(15);

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    NoAdapter,
    Stopped,
    Interrupted,
}

pub fn run(ctx: &Context, wanted: &str, interrupt: &mut dyn FnMut() -> bool) -> Outcome {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            crate::log::write(&format!("bluetooth runtime: {error}"));
            return Outcome::NoAdapter;
        }
    };
    runtime.block_on(session(ctx, wanted, interrupt))
}

async fn session(ctx: &Context, wanted: &str, interrupt: &mut dyn FnMut() -> bool) -> Outcome {
    let adapter = match Manager::new().await {
        Ok(manager) => match manager.adapters().await {
            Ok(list) => list.into_iter().next(),
            Err(error) => {
                crate::log::write(&format!("bluetooth adapters: {error}"));
                None
            }
        },
        Err(error) => {
            crate::log::write(&format!("bluetooth manager: {error}"));
            None
        }
    };
    let Some(adapter) = adapter else {
        return Outcome::NoAdapter;
    };
    loop {
        match find(ctx, &adapter, wanted, interrupt).await {
            Ok(Some((peripheral, label))) => stream(ctx, &adapter, peripheral, &label).await,
            Ok(None) if ctx.stopped() => return Outcome::Stopped,
            Ok(None) => return Outcome::Interrupted,
            Err(error) => {
                let _ = crate::log::write_changed(&format!("bluetooth scan: {error}"));
                ctx.status("Bluetooth is off or unavailable");
                if pause(ctx, Duration::from_secs(5)).await {
                    return Outcome::Stopped;
                }
                if interrupt() {
                    return Outcome::Interrupted;
                }
            }
        }
        if ctx.stopped() {
            return Outcome::Stopped;
        }
    }
}

pub fn matches(device: &Device, wanted: &str) -> bool {
    let wanted = wanted.trim();
    wanted.is_empty()
        || device.address.eq_ignore_ascii_case(wanted)
        || device.name.to_lowercase().contains(&wanted.to_lowercase())
}

async fn find(
    ctx: &Context,
    adapter: &Adapter,
    wanted: &str,
    interrupt: &mut dyn FnMut() -> bool,
) -> btleplug::Result<Option<(Peripheral, String)>> {
    ctx.status("Searching for a heart rate device");
    let filter = if wanted.is_empty() {
        ScanFilter {
            services: vec![HR_SERVICE],
        }
    } else {
        ScanFilter::default()
    };
    adapter.start_scan(filter).await?;
    let mut shown: Vec<Device> = Vec::new();
    let found = loop {
        if pause(ctx, STEP).await || interrupt() {
            break None;
        }
        let mut seen = Vec::new();
        let mut best: Option<(Peripheral, Device, i16)> = None;
        for peripheral in adapter.peripherals().await? {
            let Ok(Some(props)) = peripheral.properties().await else {
                continue;
            };
            let device = Device {
                name: props.local_name.unwrap_or_default(),
                address: props.address.to_string(),
            };
            let heart_rate = wanted.is_empty() || props.services.contains(&HR_SERVICE);
            if !heart_rate && !(matches(&device, wanted) && !device.name.is_empty()) {
                continue;
            }
            seen.push(device.clone());
            let rssi = props.rssi.unwrap_or(i16::MIN);
            if matches(&device, wanted) && best.as_ref().is_none_or(|b| rssi > b.2) {
                best = Some((peripheral, device, rssi));
            }
        }
        seen.sort_by(|a, b| a.name.cmp(&b.name).then(a.address.cmp(&b.address)));
        if seen != shown {
            ctx.devices(seen.clone());
            shown = seen;
        }
        if let Some((peripheral, device, _)) = best {
            let label = if device.name.is_empty() {
                device.address
            } else {
                device.name
            };
            break Some((peripheral, label));
        }
    };
    let _ = adapter.stop_scan().await;
    Ok(found)
}

async fn stream(ctx: &Context, adapter: &Adapter, peripheral: Peripheral, label: &str) {
    ctx.status(format!("Connecting to {label}"));
    crate::log::write(&format!("bluetooth connecting to {label}"));
    match subscribe(ctx, &peripheral).await {
        Some(Ok(())) => {}
        Some(Err(error)) => {
            crate::log::write(&format!("bluetooth {label}: {error}"));
            ctx.status(format!("Could not connect to {label}"));
            let _ = peripheral.disconnect().await;
            pause(ctx, Duration::from_secs(3)).await;
            return;
        }
        None => {
            let _ = peripheral.disconnect().await;
            return;
        }
    }
    let (Ok(mut notes), Ok(mut events)) =
        (peripheral.notifications().await, adapter.events().await)
    else {
        let _ = peripheral.disconnect().await;
        return;
    };
    ctx.status(format!("Bluetooth: {label}"));
    crate::log::write(&format!("bluetooth streaming from {label}"));
    let id = peripheral.id();
    let mut last = Instant::now();
    let mut no_contact = false;
    loop {
        let check = tokio::time::Instant::from_std(last + SILENCE_CHECK);
        tokio::select! {
            note = notes.next() => match note {
                Some(note) if note.uuid == HR_MEASUREMENT => {
                    last = Instant::now();
                    let Some(m) = heart_rate::parse_measurement(&note.value) else { continue };
                    match heart_rate::usable(m) {
                        Some(bpm) => {
                            if no_contact {
                                ctx.status(format!("Bluetooth: {label}"));
                                no_contact = false;
                            }
                            ctx.reading(bpm);
                        }
                        None if m.contact == Some(false) && !no_contact => {
                            ctx.status(format!("{label}: no skin contact"));
                            no_contact = true;
                        }
                        None => {}
                    }
                }
                Some(_) => {}
                None => break,
            },
            event = events.next() => match event {
                Some(CentralEvent::DeviceDisconnected(gone)) if gone == id => break,
                None => break,
                _ => {}
            },
            _ = ctx.stop_signal() => {
                let _ = peripheral.disconnect().await;
                return;
            }
            _ = tokio::time::sleep_until(check) => {
                if !peripheral.is_connected().await.unwrap_or(false) {
                    break;
                }
                last = Instant::now();
            }
        }
    }
    crate::log::write(&format!("bluetooth lost {label}"));
    ctx.status(format!("Lost {label}, reconnecting"));
    let _ = peripheral.disconnect().await;
    pause(ctx, Duration::from_secs(2)).await;
}

async fn subscribe(ctx: &Context, peripheral: &Peripheral) -> Option<btleplug::Result<()>> {
    let work = async {
        peripheral.connect().await?;
        peripheral.discover_services().await?;
        let characteristic = peripheral
            .characteristics()
            .into_iter()
            .find(|c| c.uuid == HR_MEASUREMENT)
            .ok_or_else(|| btleplug::Error::Other("no heart rate characteristic".into()))?;
        peripheral.subscribe(&characteristic).await
    };
    match guarded(ctx, tokio::time::timeout(CONNECT_TIMEOUT, work)).await? {
        Ok(result) => Some(result),
        Err(_) => Some(Err(btleplug::Error::TimedOut(CONNECT_TIMEOUT))),
    }
}

async fn guarded<F: Future>(ctx: &Context, work: F) -> Option<F::Output> {
    tokio::select! {
        out = work => Some(out),
        _ = ctx.stop_signal() => None,
    }
}

async fn pause(ctx: &Context, duration: Duration) -> bool {
    tokio::select! {
        _ = tokio::time::sleep(duration) => ctx.stopped(),
        _ = ctx.stop_signal() => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(name: &str, address: &str) -> Device {
        Device {
            name: name.into(),
            address: address.into(),
        }
    }

    #[test]
    fn matching_by_name_or_address() {
        let strap = device("COOSPO HW807", "C0:FF:EE:00:11:22");
        assert!(matches(&strap, ""));
        assert!(matches(&strap, "coospo"));
        assert!(matches(&strap, "c0:ff:ee:00:11:22"));
        assert!(!matches(&strap, "polar"));
        assert!(!matches(&strap, "C0:FF:EE:00:11:23"));
    }

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap()
    }

    #[test]
    fn a_pause_runs_its_full_length_without_a_stop() {
        let (ctx, _rx, _stop) = Context::test();
        let start = Instant::now();
        assert!(!runtime().block_on(pause(&ctx, Duration::from_millis(60))));
        assert!(start.elapsed() >= Duration::from_millis(60));
    }

    #[test]
    fn a_stop_ends_a_pause_and_guarded_work_at_once() {
        let (ctx, _rx, stop) = Context::test();
        let worker = std::thread::spawn(move || {
            let rt = runtime();
            let start = Instant::now();
            assert!(rt.block_on(pause(&ctx, Duration::from_secs(30))));
            let guarded_out = rt.block_on(guarded(&ctx, std::future::pending::<()>()));
            assert!(guarded_out.is_none());
            start.elapsed()
        });
        std::thread::sleep(Duration::from_millis(20));
        stop.stop();
        assert!(worker.join().unwrap() < Duration::from_secs(1));
    }

    #[test]
    fn uuids_are_the_standard_ones() {
        assert_eq!(
            HR_SERVICE.to_string(),
            "0000180d-0000-1000-8000-00805f9b34fb"
        );
        assert_eq!(
            HR_MEASUREMENT.to_string(),
            "00002a37-0000-1000-8000-00805f9b34fb"
        );
    }
}
