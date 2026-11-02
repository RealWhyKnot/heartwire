use std::time::Duration;

use super::{Settings, Timing, render, report, run_all};

fn median(timings: &[Timing], name: &str) -> Result<Duration, String> {
    timings
        .iter()
        .find(|timing| timing.name == name)
        .map(|timing| timing.median)
        .ok_or_else(|| format!("performance case {name} was not timed"))
}

pub(super) fn run() -> Result<(), String> {
    let timings = run_all(Settings::QUICK)?;
    if timings.len() < 30 {
        return Err(format!("{} cases", timings.len()));
    }
    let mut names: Vec<(&str, &str)> = timings
        .iter()
        .map(|timing| (timing.group, timing.name))
        .collect();
    names.sort();
    names.dedup();
    if names.len() != timings.len() {
        return Err("case names are not unique".into());
    }
    for timing in &timings {
        if timing.calls < 3 || timing.min > timing.median || timing.median > timing.p95 {
            return Err(format!("{} has invalid samples", timing.name));
        }
    }
    let mut text = Vec::new();
    report(&timings, &mut text)
        .map_err(|error| format!("format the performance report: {error}"))?;
    let text = String::from_utf8(text)
        .map_err(|error| format!("read the performance report as UTF-8: {error}"))?;
    if !text.contains("osc") || text.lines().count() != timings.len() + 3 {
        return Err("the performance report is incomplete".into());
    }
    let timings = render::timings(Settings {
        per_case: Duration::from_millis(20),
        samples: 5,
    });
    let shown = median(&timings, "ping animation frame")?;
    let hidden = median(&timings, "ping frame while minimized")?;
    if hidden * 20 >= shown {
        return Err(format!(
            "a minimized tick took {hidden:?}, an on-screen ping frame {shown:?}"
        ));
    }
    let partial = median(&timings, "VR panel frame after a bpm change")?;
    let fresh = median(&timings, "VR panel create and first frame")?;
    if partial * 2 >= fresh {
        return Err(format!(
            "a partial panel frame took {partial:?}, a fresh one {fresh:?}"
        ));
    }
    Ok(())
}
