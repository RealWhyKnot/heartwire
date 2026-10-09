use std::cell::RefCell;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_BYTES: u64 = 512 * 1024;

static LOG: Mutex<Option<Log>> = Mutex::new(None);

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
        if self.written + line.len() as u64 > self.max {
            self.rotate();
        }
        if let Some(file) = self.file.as_mut()
            && file.write_all(line.as_bytes()).is_ok()
        {
            self.written += line.len() as u64;
        }
    }
}

pub fn init(dir: &Path) {
    *LOG.lock().unwrap_or_else(|e| e.into_inner()) = Some(Log::open(dir, MAX_BYTES));
}

pub fn write(message: &str) {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let line = format!("{} {message}\n", timestamp(secs));
    if cfg!(debug_assertions) {
        eprint!("{line}");
    }
    if let Some(log) = LOG.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        log.line(&line);
    }
}

pub fn write_changed(message: &str) -> bool {
    LAST.with(|last| {
        let mut last = last.borrow_mut();
        if *last == message {
            return false;
        }
        write(message);
        message.clone_into(&mut last);
        true
    })
}

pub fn timestamp(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
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
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

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
