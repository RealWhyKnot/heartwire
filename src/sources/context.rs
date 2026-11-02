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

pub(crate) enum Wake {
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
pub(crate) struct Context {
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

    pub(crate) fn reading(&self, bpm: u16) {
        let _ = self.tx.send(Event::Reading {
            generation: self.generation,
            bpm,
        });
    }

    pub(crate) fn status(&self, text: impl Into<String>) {
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

    pub(crate) fn devices(&self, list: Vec<Device>) {
        let _ = self.tx.send(Event::Devices {
            generation: self.generation,
            list,
        });
    }

    pub(crate) fn stopped(&self) -> bool {
        self.shared.stopped.load(Ordering::Acquire)
    }

    pub(crate) fn sleep(&self, duration: Duration) -> bool {
        self.sleep_after(duration, || {})
    }

    fn sleep_after(&self, duration: Duration, before_wait: impl FnOnce()) -> bool {
        let mut before_wait = Some(before_wait);
        let _ = self
            .shared
            .signal
            .wait_timeout_while(lock(&self.shared.lock), duration, |_| {
                if let Some(before_wait) = before_wait.take() {
                    before_wait();
                }
                !self.stopped()
            });
        self.stopped()
    }

    pub(crate) async fn stop_signal(&self) {
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

    pub(crate) fn set_wake(&self, wake: Wake) {
        let mut slot = lock(&self.shared.lock);
        *slot = wake;
        if self.stopped() {
            fire(&mut slot);
        }
        drop(slot);
    }

    pub(super) fn stopper(&self) -> Stopper {
        Stopper {
            shared: std::sync::Arc::clone(&self.shared),
        }
    }

    pub(crate) fn standalone() -> (Context, std::sync::mpsc::Receiver<Event>, Stopper) {
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = Context::new(1, tx);
        let stopper = ctx.stopper();
        (ctx, rx, stopper)
    }

    #[cfg(test)]
    pub(crate) fn test() -> (Context, std::sync::mpsc::Receiver<Event>, Stopper) {
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

pub(crate) struct Stopper {
    shared: Arc<Shared>,
}

impl Stopper {
    pub(crate) fn stop(&self) {
        self.shared.stopped.store(true, Ordering::Release);
        let mut slot = lock(&self.shared.lock);
        fire(&mut slot);
        self.shared.signal.notify_all();
        drop(slot);
        self.shared.waker.wake();
    }
}

pub(crate) struct Handle {
    pub(super) stopper: Stopper,
    pub(super) thread: Option<JoinHandle<()>>,
}

impl Handle {
    pub(crate) fn stop(mut self) {
        self.stopper.stop();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::Future;

    fn statuses(rx: &std::sync::mpsc::Receiver<Event>) -> Vec<String> {
        rx.try_iter()
            .filter_map(|e| match e {
                Event::Status { text, .. } => Some(text),
                Event::Reading { .. }
                | Event::Devices { .. }
                | Event::Config(_)
                | Event::Restart
                | Event::InstallFirmware
                | Event::Quit => None,
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
        let (waiting, started) = std::sync::mpsc::channel();
        let (finished, done) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            assert!(ctx.sleep_after(Duration::from_secs(30), || {
                waiting.send(()).unwrap();
            }));
            finished.send(()).unwrap();
        });
        started.recv().unwrap();
        stop.stop();
        done.recv_timeout(Duration::from_secs(1)).unwrap();
        worker.join().unwrap();
    }

    #[test]
    fn stop_wakes_the_async_signal_at_once() {
        let (ctx, _rx, stop) = Context::test();
        let mut signal = std::pin::pin!(ctx.stop_signal());
        let mut task = std::task::Context::from_waker(std::task::Waker::noop());
        assert!(signal.as_mut().poll(&mut task).is_pending());
        stop.stop();
        assert!(signal.as_mut().poll(&mut task).is_ready());
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
