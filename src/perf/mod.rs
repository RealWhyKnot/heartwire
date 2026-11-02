use num_traits::ToPrimitive;

mod cases;
mod check;
mod render;

use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::version::{self, Channel};

pub(crate) const FLAG: &str = "--perf";

#[derive(Clone, Copy)]
pub(crate) struct Settings {
    pub per_case: Duration,
    pub samples: u32,
}

impl Settings {
    pub(crate) const FULL: Settings = Settings {
        per_case: Duration::from_millis(300),
        samples: 25,
    };

    pub(crate) const QUICK: Settings = Settings {
        per_case: Duration::from_micros(300),
        samples: 3,
    };
}

pub(crate) struct Case {
    group: &'static str,
    name: &'static str,
    budget: Duration,
    run: Box<dyn FnMut()>,
}

impl Case {
    pub(crate) fn new(
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
pub(crate) struct Timing {
    pub group: &'static str,
    pub name: &'static str,
    pub budget: Duration,
    pub calls: u64,
    pub median: Duration,
    pub p95: Duration,
    pub min: Duration,
}

impl Timing {
    pub(crate) fn over_budget(&self) -> bool {
        self.median > self.budget
    }
}

pub(crate) fn time(case: &mut Case, settings: Settings) -> Timing {
    time_with(case, settings, Instant::now)
}

fn time_with(case: &mut Case, settings: Settings, mut now: impl FnMut() -> Instant) -> Timing {
    let run = &mut case.run;
    run();
    let start = now();
    run();
    let one = now().duration_since(start).max(Duration::from_nanos(1));
    let target = settings.per_case / settings.samples.max(1);
    let batch = u32::try_from((target.as_nanos() / one.as_nanos()).clamp(1, 1_000_000))
        .expect("batch size is at most one million");
    let mut per_call: Vec<Duration> = (0..settings.samples.max(1))
        .map(|_| {
            let start = now();
            for _ in 0..batch {
                run();
            }
            now().duration_since(start) / batch
        })
        .collect();
    per_call.sort();
    let n = per_call.len();
    Timing {
        group: case.group,
        name: case.name,
        budget: case.budget,
        calls: u64::from(batch) * u64::try_from(n).expect("the sample count comes from a u32") + 2,
        median: per_call[n / 2],
        p95: per_call[(n * 95 / 100).min(n - 1)],
        min: per_call[0],
    }
}

pub(crate) fn run_all(settings: Settings) -> Result<Vec<Timing>, String> {
    let mut timings: Vec<Timing> = cases::all()?
        .iter_mut()
        .map(|case| time(case, settings))
        .collect();
    timings.extend(render::timings(settings));
    Ok(timings)
}

pub(crate) fn format_duration(d: Duration) -> String {
    let ns = d.as_nanos();
    match ns {
        0..1_000 => format!("{ns} ns"),
        1_000..1_000_000 => format!(
            "{:.1} us",
            ns.to_f64().expect("Duration nanoseconds fit the f64 range") / 1e3
        ),
        1_000_000..1_000_000_000 => format!(
            "{:.2} ms",
            ns.to_f64().expect("Duration nanoseconds fit the f64 range") / 1e6
        ),
        _ => format!(
            "{:.2} s",
            ns.to_f64().expect("Duration nanoseconds fit the f64 range") / 1e9
        ),
    }
}

pub(crate) fn report(timings: &[Timing], out: &mut dyn Write) -> std::io::Result<()> {
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

pub(crate) fn requested() -> Option<Option<PathBuf>> {
    let mut args = std::env::args().skip_while(|a| a != FLAG);
    args.next()?;
    Some(
        args.next()
            .filter(|a| !a.starts_with("--"))
            .map(PathBuf::from),
    )
}

pub(crate) fn main(target: Option<PathBuf>) -> Result<(), String> {
    let timings = run_all(Settings::FULL)?;
    let mut text = Vec::new();
    report(&timings, &mut text)
        .map_err(|error| format!("format the performance report: {error}"))?;
    match target {
        Some(path) => std::fs::write(&path, &text)
            .map_err(|error| format!("writing {}: {error}", path.display())),
        None => std::io::stdout()
            .lock()
            .write_all(&text)
            .map_err(|error| format!("write the performance report to stdout: {error}")),
    }
}

pub fn check_performance() -> Result<(), String> {
    check::run()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct BrokenOutput;

    impl Write for BrokenOutput {
        fn write(&mut self, _buffer: &[u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_report_returns_output_failures() {
        assert_eq!(
            report(&[], &mut BrokenOutput).unwrap_err().kind(),
            std::io::ErrorKind::BrokenPipe
        );
    }

    #[test]
    fn durations_read_well() {
        assert_eq!(format_duration(Duration::from_nanos(850)), "850 ns");
        assert_eq!(format_duration(Duration::from_nanos(12_340)), "12.3 us");
        assert_eq!(format_duration(Duration::from_micros(4_560)), "4.56 ms");
        assert_eq!(format_duration(Duration::from_millis(2_500)), "2.50 s");
    }

    #[test]
    fn a_slow_case_is_flagged() {
        let elapsed = std::rc::Rc::new(std::cell::Cell::new(Duration::ZERO));
        let clock = std::rc::Rc::clone(&elapsed);
        let mut case = Case::new("test", "sleep", Duration::from_micros(1), move || {
            elapsed.set(elapsed.get() + Duration::from_millis(1));
        });
        let start = Instant::now();
        let timing = time_with(&mut case, Settings::QUICK, || start + clock.get());
        assert!(timing.over_budget());
        assert_eq!(timing.median, Duration::from_millis(1));
    }
}
