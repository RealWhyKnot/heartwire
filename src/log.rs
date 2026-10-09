use std::cell::RefCell;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_BYTES: u64 = 512 * 1024;

static FILE: Mutex<Option<File>> = Mutex::new(None);

thread_local! {
    static LAST: RefCell<String> = const { RefCell::new(String::new()) };
}

pub fn init(dir: &Path) {
    let path = dir.join("heartwire.log");
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > MAX_BYTES) {
        let _ = std::fs::rename(&path, dir.join("heartwire.old.log"));
    }
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .ok();
    *FILE.lock().unwrap_or_else(|e| e.into_inner()) = file;
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
    if let Some(file) = FILE.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        let _ = file.write_all(line.as_bytes());
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
    #[test]
    fn timestamps() {
        assert_eq!(super::timestamp(0), "1970-01-01 00:00:00Z");
        assert_eq!(super::timestamp(951_782_400), "2000-02-29 00:00:00Z");
        assert_eq!(
            super::timestamp(1_791_504_000 + 3_723),
            "2026-10-09 01:02:03Z"
        );
    }

    #[test]
    fn repeats_are_written_once() {
        assert!(super::write_changed("pico: no strap found"));
        assert!(!super::write_changed("pico: no strap found"));
        assert!(super::write_changed("pico: subscribed"));
        assert!(super::write_changed("pico: no strap found"));
    }
}
