mod cases;
mod render;

use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::version::{self, Channel};

pub const FLAG: &str = "--perf";

#[derive(Clone, Copy)]
pub struct Settings {
    pub per_case: Duration,
    pub samples: u32,
}

impl Settings {
    pub const FULL: Settings = Settings {
        per_case: Duration::from_millis(300),
        samples: 25,
    };

    #[cfg(test)]
    pub const QUICK: Settings = Settings {
        per_case: Duration::from_micros(300),
        samples: 3,
    };
}

pub struct Case {
    group: &'static str,
    name: &'static str,
    budget: Duration,
    run: Box<dyn FnMut()>,
}

impl Case {
    pub fn new(
        group: &'static str,
        name: &'static str,
        budget: Duration,
        run: impl FnMut() + 'static,
    ) -> Case {
        Case {
            group,
            name,
            budget,
            run: Box::new(run),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Timing {
    pub group: &'static str,
    pub name: &'static str,
    pub budget: Duration,
    pub calls: u64,
    pub median: Duration,
    pub p95: Duration,
    pub min: Duration,
}

impl Timing {
    pub fn over_budget(&self) -> bool {
        self.median > self.budget
    }
}

pub fn time(case: &mut Case, settings: Settings) -> Timing {
    let run = &mut case.run;
    run();
    let start = Instant::now();
    run();
    let one = start.elapsed().max(Duration::from_nanos(1));
    let target = settings.per_case / settings.samples.max(1);
    let batch = (target.as_nanos() / one.as_nanos()).clamp(1, 1_000_000) as u32;
    let mut per_call: Vec<Duration> = (0..settings.samples.max(1))
        .map(|_| {
            let start = Instant::now();
            for _ in 0..batch {
                run();
            }
            start.elapsed() / batch
        })
        .collect();
    per_call.sort();
    let n = per_call.len();
    Timing {
        group: case.group,
        name: case.name,
        budget: case.budget,
        calls: u64::from(batch) * n as u64 + 2,
        median: per_call[n / 2],
        p95: per_call[(n * 95 / 100).min(n - 1)],
        min: per_call[0],
    }
}

pub fn run_all(settings: Settings) -> Vec<Timing> {
    let mut timings: Vec<Timing> = cases::all()
        .iter_mut()
        .map(|case| time(case, settings))
        .collect();
    timings.extend(render::timings(settings));
    timings
}

pub fn format_duration(d: Duration) -> String {
    let ns = d.as_nanos();
    match ns {
        0..1_000 => format!("{ns} ns"),
        1_000..1_000_000 => format!("{:.1} us", ns as f64 / 1e3),
        1_000_000..1_000_000_000 => format!("{:.2} ms", ns as f64 / 1e6),
        _ => format!("{:.2} s", ns as f64 / 1e9),
    }
}

pub fn report(timings: &[Timing], out: &mut dyn Write) -> std::io::Result<()> {
    writeln!(
        out,
        "Heartwire {} ({}) {} performance, {} build",
        version::VERSION,
        Channel::current().name(),
        crate::update::rid(),
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }
    )?;
    writeln!(
        out,
        "{:<10} {:<44} {:>10} {:>10} {:>10} {:>10} {:>10}",
        "group", "case", "median", "p95", "min", "budget", "calls"
    )?;
    for t in timings {
        writeln!(
            out,
            "{:<10} {:<44} {:>10} {:>10} {:>10} {:>10} {:>10}{}",
            t.group,
            t.name,
            format_duration(t.median),
            format_duration(t.p95),
            format_duration(t.min),
            format_duration(t.budget),
            t.calls,
            if t.over_budget() { "  over budget" } else { "" }
        )?;
    }
    let over = timings.iter().filter(|t| t.over_budget()).count();
    writeln!(out, "{} cases, {over} over budget", timings.len())
}

pub fn requested() -> Option<Option<PathBuf>> {
    let mut args = std::env::args().skip_while(|a| a != FLAG);
    args.next()?;
    Some(
        args.next()
            .filter(|a| !a.starts_with("--"))
            .map(PathBuf::from),
    )
}

pub fn main(target: Option<PathBuf>) {
    let timings = run_all(Settings::FULL);
    let mut text = Vec::new();
    let _ = report(&timings, &mut text);
    match target {
        Some(path) => {
            if let Err(error) = std::fs::write(&path, &text) {
                eprintln!("writing {}: {error}", path.display());
            }
        }
        None => {
            let _ = std::io::stdout().write_all(&text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_case_runs_and_reports() {
        let timings = run_all(Settings::QUICK);
        assert!(timings.len() >= 30, "{} cases", timings.len());
        let mut names: Vec<(&str, &str)> = timings.iter().map(|t| (t.group, t.name)).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), timings.len(), "case names are unique");
        for t in &timings {
            assert!(t.calls >= 3, "{} ran", t.name);
            assert!(
                t.min <= t.median && t.median <= t.p95,
                "{} is ordered",
                t.name
            );
        }
        let mut text = Vec::new();
        report(&timings, &mut text).unwrap();
        let text = String::from_utf8(text).unwrap();
        assert!(text.contains("osc"));
        assert!(text.lines().count() == timings.len() + 3);
    }

    #[test]
    fn durations_read_well() {
        assert_eq!(format_duration(Duration::from_nanos(850)), "850 ns");
        assert_eq!(format_duration(Duration::from_nanos(12_340)), "12.3 us");
        assert_eq!(format_duration(Duration::from_micros(4_560)), "4.56 ms");
        assert_eq!(format_duration(Duration::from_millis(2_500)), "2.50 s");
    }

    #[test]
    fn hidden_and_partial_frames_stay_cheaper_than_full_ones() {
        let timings = render::timings(Settings {
            per_case: Duration::from_millis(20),
            samples: 5,
        });
        let median = |name: &str| {
            timings
                .iter()
                .find(|t| t.name == name)
                .unwrap_or_else(|| panic!("{name} was timed"))
                .median
        };
        let shown = median("ping animation frame");
        let hidden = median("ping frame while minimized");
        assert!(
            hidden * 20 < shown,
            "a minimized tick took {hidden:?}, an on-screen ping frame {shown:?}"
        );
        let partial = median("VR panel frame after a bpm change");
        let fresh = median("VR panel create and first frame");
        assert!(
            partial * 2 < fresh,
            "a partial panel frame took {partial:?}, a fresh one {fresh:?}"
        );
    }

    #[test]
    fn a_slow_case_is_flagged() {
        let mut case = Case::new("test", "sleep", Duration::from_micros(1), || {
            std::thread::sleep(Duration::from_millis(1));
        });
        let timing = time(&mut case, Settings::QUICK);
        assert!(timing.over_budget());
    }
}
