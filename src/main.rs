#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    tracing::subscriber::set_global_default(heartwire::subscriber())
        .expect("main installs the subscriber before spawning threads");
    heartwire::run();
}
