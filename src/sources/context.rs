use std::future::poll_fn;
use std::net::{SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::task::Poll;
use std::thread::JoinHandle;
use std::time::Duration;

use futures::task::AtomicWaker;

use super::Device;
use crate::engine::Event;

pub enum Wake {
    None,
    Connect(SocketAddr),
    Shutdown(TcpStream),
}

struct Shared {
    stopped: AtomicBool,
    lock: Mutex<Wake>,
    signal: Condvar,
    waker: AtomicWaker,
    status: Mutex<String>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Clone)]
pub struct Context {
    generation: u64,
    tx: Sender<Event>,
    shared: Arc<Shared>,
}

impl Context {
    pub(super) fn new(generation: u64, tx: Sender<Event>) -> Self {
        Context {
            generation,
            tx,
            shared: Arc::new(Shared {
                stopped: AtomicBool::new(false),
                lock: Mutex::new(Wake::None),
                signal: Condvar::new(),
                waker: AtomicWaker::new(),
                status: Mutex::new(String::new()),
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
        let text = text.into();
        {
            let mut last = lock(&self.shared.status);
            if *last == text {
                return;
            }
            last.clone_from(&text);
        }
        let _ = self.tx.send(Event::Status {
            generation: self.generation,
            text,
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
        let guard = lock(&self.shared.lock);
        let _ = self
            .shared
            .signal
            .wait_timeout_while(guard, duration, |_| !self.stopped());
        self.stopped()
    }

    pub async fn stop_signal(&self) {
        poll_fn(|cx| {
            if self.stopped() {
                return Poll::Ready(());
            }
            self.shared.waker.register(cx.waker());
            if self.stopped() {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        })
        .await;
    }

    pub fn set_wake(&self, wake: Wake) {
        let mut slot = lock(&self.shared.lock);
        *slot = wake;
        if self.stopped() {
            fire(&mut slot);
        }
    }

    pub(super) fn stopper(&self) -> Stopper {
        Stopper {
            shared: self.shared.clone(),
        }
    }

    pub fn standalone() -> (Context, std::sync::mpsc::Receiver<Event>, Stopper) {
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = Context::new(1, tx);
        let stopper = ctx.stopper();
        (ctx, rx, stopper)
    }

    #[cfg(test)]
    pub fn test() -> (Context, std::sync::mpsc::Receiver<Event>, Stopper) {
        Context::standalone()
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
        let mut slot = lock(&self.shared.lock);
        fire(&mut slot);
        self.shared.signal.notify_all();
        drop(slot);
        self.shared.waker.wake();
    }
}

pub struct Handle {
    pub(super) stopper: Stopper,
    pub(super) thread: Option<JoinHandle<()>>,
}

impl Handle {
    pub fn stop(mut self) {
        self.stopper.stop();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn statuses(rx: &std::sync::mpsc::Receiver<Event>) -> Vec<String> {
        rx.try_iter()
            .filter_map(|e| match e {
                Event::Status { text, .. } => Some(text),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_repeated_status_is_sent_once() {
        let (ctx, rx, _stop) = Context::test();
        ctx.status("Waiting for a Pico");
        ctx.status("Waiting for a Pico");
        ctx.status("Pico on COM5");
        ctx.status("Waiting for a Pico");
        assert_eq!(
            statuses(&rx),
            ["Waiting for a Pico", "Pico on COM5", "Waiting for a Pico"]
        );
    }

    #[test]
    fn stop_wakes_a_sleeping_source_at_once() {
        let (ctx, _rx, stop) = Context::test();
        let worker = std::thread::spawn(move || {
            let start = Instant::now();
            assert!(ctx.sleep(Duration::from_secs(30)));
            start.elapsed()
        });
        std::thread::sleep(Duration::from_millis(20));
        stop.stop();
        assert!(worker.join().unwrap() < Duration::from_secs(1));
    }

    #[test]
    fn stop_wakes_the_async_signal_at_once() {
        let (ctx, _rx, stop) = Context::test();
        let worker = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .build()
                .unwrap();
            let start = Instant::now();
            runtime.block_on(async {
                tokio::select! {
                    _ = ctx.stop_signal() => {}
                    _ = tokio::time::sleep(Duration::from_secs(30)) => panic!("stop was missed"),
                }
            });
            start.elapsed()
        });
        std::thread::sleep(Duration::from_millis(20));
        stop.stop();
        assert!(worker.join().unwrap() < Duration::from_secs(1));
    }

    #[test]
    fn the_async_signal_is_ready_after_a_stop() {
        let (ctx, _rx, stop) = Context::test();
        stop.stop();
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(ctx.stop_signal());
    }
}
