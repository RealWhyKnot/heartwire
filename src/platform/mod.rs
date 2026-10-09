#[cfg(not(windows))]
mod other;
#[cfg(windows)]
mod windows;

use std::path::PathBuf;

#[cfg(not(windows))]
use other as imp;
#[cfg(windows)]
use windows as imp;

pub use imp::{focus_existing, open_url, serial_ports_key};

pub const MINIMIZED_FLAG: &str = "--minimized";

fn launch_command() -> Option<(PathBuf, String)> {
    let exe = std::env::current_exe().ok()?;
    let line = format!("\"{}\" {MINIMIZED_FLAG}", exe.display());
    Some((exe, line))
}

pub fn autostart_enabled() -> bool {
    imp::autostart_value().is_some()
}

pub fn set_autostart(on: bool) -> Result<(), String> {
    if on {
        let (_, line) = launch_command().ok_or("can't find the program path")?;
        imp::set_autostart(Some(&line))
    } else {
        imp::set_autostart(None)
    }
}

pub fn refresh_autostart() {
    let Some(stored) = imp::autostart_value() else {
        return;
    };
    if let Some((_, line)) = launch_command()
        && stored != line
    {
        match imp::set_autostart(Some(&line)) {
            Ok(()) => crate::log::write("autostart now points at this copy"),
            Err(error) => crate::log::write(&format!("autostart refresh: {error}")),
        }
    }
}
