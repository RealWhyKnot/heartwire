#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

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

fn main() {
    app::run();
}
