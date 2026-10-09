use std::time::{Duration, Instant};

const REFRESH: Duration = Duration::from_secs(30);

#[derive(Default)]
pub struct PortWatch {
    key: Option<Vec<u16>>,
    found: Option<String>,
    at: Option<Instant>,
}

impl PortWatch {
    pub fn find(&mut self) -> Option<String> {
        self.find_with(
            crate::platform::serial_ports_key(),
            Instant::now(),
            super::find_port,
        )
    }

    pub fn forget(&mut self) {
        self.at = None;
    }

    fn find_with(
        &mut self,
        key: Option<Vec<u16>>,
        now: Instant,
        enumerate: impl FnOnce() -> Option<String>,
    ) -> Option<String> {
        let fresh = self.at.is_some_and(|at| now.duration_since(at) < REFRESH);
        if fresh && key.is_some() && key == self.key {
            return self.found.clone();
        }
        self.found = enumerate();
        self.key = key;
        self.at = Some(now);
        self.found.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn ports_are_enumerated_only_when_the_port_list_changes_or_goes_stale() {
        let calls = Cell::new(0);
        let enumerate = || {
            calls.set(calls.get() + 1);
            Some("COM5".to_owned())
        };
        let mut watch = PortWatch::default();
        let start = Instant::now();
        let one = Some(vec![1u16]);
        assert_eq!(
            watch.find_with(one.clone(), start, enumerate).as_deref(),
            Some("COM5")
        );
        for second in 1..29 {
            let at = start + Duration::from_secs(second);
            assert_eq!(
                watch.find_with(one.clone(), at, enumerate).as_deref(),
                Some("COM5")
            );
        }
        assert_eq!(
            calls.get(),
            1,
            "an unchanged port list is not enumerated again"
        );
        watch.find_with(Some(vec![1, 2]), start + Duration::from_secs(29), enumerate);
        assert_eq!(calls.get(), 2, "a changed port list is enumerated");
        watch.find_with(Some(vec![1, 2]), start + Duration::from_secs(60), enumerate);
        assert_eq!(calls.get(), 3, "a stale answer is refreshed");
        watch.forget();
        watch.find_with(Some(vec![1, 2]), start + Duration::from_secs(61), enumerate);
        assert_eq!(calls.get(), 4, "forget forces a fresh look");
        watch.find_with(None, start + Duration::from_secs(62), enumerate);
        watch.find_with(None, start + Duration::from_secs(63), enumerate);
        assert_eq!(
            calls.get(),
            6,
            "without a port list key every call enumerates"
        );
    }

    #[test]
    #[ignore = "prints how long a port check takes on this machine"]
    fn port_check_cost() {
        let runs = 200;
        let start = Instant::now();
        for _ in 0..runs {
            std::hint::black_box(crate::platform::serial_ports_key());
        }
        let key = start.elapsed() / runs;
        let start = Instant::now();
        for _ in 0..20 {
            std::hint::black_box(super::super::find_port());
        }
        let full = start.elapsed() / 20;
        println!("port list key {key:?}, full enumeration {full:?}");
    }
}
