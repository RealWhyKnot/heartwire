mod app;
mod config;
mod engine;
mod heart_rate;
mod log;
mod net;
mod osc;
mod perf;
mod platform;
mod sources;
mod steamvr;
mod ui;
mod update;
mod version;

pub use app::run;
pub use perf::check_performance;

pub fn subscriber() -> impl tracing::Subscriber {
    log::subscriber(log::write)
}
