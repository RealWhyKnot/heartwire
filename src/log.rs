use std::cell::RefCell;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, TryLockError};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::{Layer, Registry};

const MAX_BYTES: u64 = 512 * 1024;

static LOG: Mutex<Option<Log>> = Mutex::new(None);

struct TextLayer<F>(F);

#[derive(Default)]
struct Message(String);

impl Visit for Message {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            use std::fmt::Write;
            write!(&mut self.0, "{value:?}").expect("formatting into a String cannot fail");
        }
    }
}

impl<S: Subscriber, F: Fn(&str) + Send + Sync + 'static> Layer<S> for TextLayer<F> {
    fn on_event(&self, event: &Event<'_>, _: Context<'_, S>) {
        let mut message = Message::default();
        event.record(&mut message);
        (self.0)(&message.0);
    }
}

pub(crate) fn subscriber(sink: impl Fn(&str) + Send + Sync + 'static) -> impl Subscriber {
    Registry::default().with(
        TextLayer(sink).with_filter(tracing_subscriber::filter::filter_fn(|metadata| {
            metadata.target().starts_with("heartwire")
        })),
    )
}

thread_local! {
    static LAST: RefCell<String> = const { RefCell::new(String::new()) };
}

struct Log {
    path: PathBuf,
    file: Option<File>,
    written: u64,
    max: u64,
}

fn append(path: &Path) -> Option<File> {
    OpenOptions::new().create(true).append(true).open(path).ok()
}

impl Log {
    fn open(dir: &Path, max: u64) -> Log {
        let path = dir.join("heartwire.log");
        let written = std::fs::metadata(&path).map_or(0, |m| m.len());
        let mut log = Log {
            file: append(&path),
            path,
            written,
            max,
        };
        if written > max {
            log.rotate();
        }
        log
    }

    fn rotate(&mut self) {
        self.file = None;
        let _ = std::fs::rename(&self.path, self.path.with_file_name("heartwire.old.log"));
        self.file = append(&self.path);
        self.written = 0;
    }

    fn line(&mut self, line: &str) {
        if self.written + u64::try_from(line.len()).expect("supported pointer widths fit u64")
            > self.max
        {
            self.rotate();
        }
        if let Some(file) = self.file.as_mut()
            && file.write_all(line.as_bytes()).is_ok()
        {
            self.written += u64::try_from(line.len()).expect("supported pointer widths fit u64");
        }
    }
}

pub(crate) fn init(dir: &Path) {
    *LOG.lock().unwrap_or_else(|e| e.into_inner()) = Some(Log::open(dir, MAX_BYTES));
}

pub(crate) fn catch_panics() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let line = format!(
            "{} panic in {}: {info}\n",
            now(),
            thread.name().unwrap_or("unnamed thread")
        );
        last_words(&LOG, &line);
        default(info);
    }));
}

fn last_words(slot: &Mutex<Option<Log>>, line: &str) -> bool {
    last_words_after(slot, line, || {
        std::thread::sleep(std::time::Duration::from_millis(2));
    })
}

fn last_words_after(slot: &Mutex<Option<Log>>, line: &str, mut retry: impl FnMut()) -> bool {
    for _ in 0..50 {
        let mut held = match slot.try_lock() {
            Ok(held) => held,
            Err(TryLockError::Poisoned(e)) => e.into_inner(),
            Err(TryLockError::WouldBlock) => {
                retry();
                continue;
            }
        };
        return held.as_mut().is_some_and(|log| {
            log.line(line);
            true
        });
    }
    false
}

fn now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    timestamp(secs)
}

pub(crate) fn write(message: &str) {
    let line = format!("{} {message}\n", now());
    if cfg!(debug_assertions) {
        let _ = std::io::stderr().lock().write_all(line.as_bytes());
    }
    if let Some(log) = LOG.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        log.line(&line);
    }
}

pub(crate) fn write_changed(message: &str) -> bool {
    LAST.with(|last| {
        let mut last = last.borrow_mut();
        if *last == message {
            return false;
        }
        tracing::info!("{message}");
        message.clone_into(&mut last);
        true
    })
}

pub(crate) fn timestamp(secs: u64) -> String {
    let days = i64::try_from(secs / 86_400).expect("u64 seconds divided by 86400 fit i64");
    let rem = secs % 86_400;
    let (y, m, d) = civil(days);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = u32::try_from(doy - (153 * mp + 2) / 5 + 1).expect("a day of the month is positive");
    let m =
        u32::try_from(if mp < 10 { mp + 3 } else { mp - 9 }).expect("a month is between 1 and 12");
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracing_preserves_message_text_and_omits_dependency_events() {
        let messages = std::sync::Arc::new(Mutex::new(Vec::new()));
        let captured = std::sync::Arc::clone(&messages);
        tracing::subscriber::with_default(
            subscriber(move |message| {
                captured.lock().unwrap().push(message.to_owned());
            }),
            || {
                tracing::info!(target: "heartwire::log", "pico: {}", "subscribed");
                tracing::warn!(target: "heartwire::log", "open {}: {}", "COM5", "busy");
                tracing::info!(target: "dependency", "hidden");
            },
        );
        assert_eq!(
            *messages.lock().unwrap(),
            ["pico: subscribed", "open COM5: busy"]
        );
    }

    #[test]
    fn timestamps() {
        assert_eq!(timestamp(0), "1970-01-01 00:00:00Z");
        assert_eq!(timestamp(951_782_400), "2000-02-29 00:00:00Z");
        assert_eq!(timestamp(1_791_504_000 + 3_723), "2026-10-09 01:02:03Z");
    }

    #[test]
    fn repeats_are_written_once() {
        assert!(write_changed("pico: no strap found"));
        assert!(!write_changed("pico: no strap found"));
        assert!(write_changed("pico: subscribed"));
        assert!(write_changed("pico: no strap found"));
    }

    #[test]
    fn a_panic_reaches_the_log_even_when_the_lock_is_poisoned_or_held() {
        let dir = std::env::temp_dir().join(format!("heartwire-panic-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let slot = std::sync::Arc::new(Mutex::new(Some(Log::open(&dir, MAX_BYTES))));
        let poisoner = std::sync::Arc::clone(&slot);
        let _ = std::thread::spawn(move || {
            let _held = poisoner.lock().unwrap();
            panic!("poison the log");
        })
        .join();
        assert!(slot.is_poisoned());
        assert!(last_words(&slot, "first\n"));
        let busy = std::sync::Arc::clone(&slot);
        let (acquired, locked) = std::sync::mpsc::channel();
        let (release, released) = std::sync::mpsc::channel();
        let (unlocked, ready) = std::sync::mpsc::channel();
        let holder = std::thread::spawn(move || {
            let held = busy.lock().unwrap_or_else(|e| e.into_inner());
            acquired.send(()).unwrap();
            released.recv().unwrap();
            drop(held);
            unlocked.send(()).unwrap();
        });
        locked.recv().unwrap();
        let mut retries = 0;
        assert!(last_words_after(&slot, "second\n", || {
            retries += 1;
            release.send(()).unwrap();
            ready.recv().unwrap();
        }));
        assert_eq!(retries, 1);
        holder.join().unwrap();
        assert!(!last_words(&Mutex::new(None), "nowhere\n"));
        drop(slot);
        let text = std::fs::read_to_string(dir.join("heartwire.log")).unwrap();
        assert_eq!(text, "first\nsecond\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_long_running_log_rotates_instead_of_growing() {
        let dir = std::env::temp_dir().join(format!("heartwire-log-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut log = Log::open(&dir, 100);
        for i in 0..12 {
            log.line(&format!("line {i:02} with some padding\n"));
        }
        drop(log);
        let current = std::fs::metadata(dir.join("heartwire.log")).unwrap().len();
        let old = std::fs::read_to_string(dir.join("heartwire.old.log")).unwrap();
        assert!(
            current <= 100,
            "the live log stays under the cap ({current} bytes)"
        );
        assert!(old.len() <= 100 && old.starts_with("line "));
        let reopened = Log::open(&dir, 10);
        assert_eq!(reopened.written, 0, "an oversized log rotates at start-up");
        drop(reopened);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
