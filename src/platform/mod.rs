#[cfg(not(windows))]
mod other;
#[cfg(windows)]
mod windows;

use std::path::PathBuf;

#[cfg(not(windows))]
use other as imp;
#[cfg(windows)]
use windows as imp;

pub(crate) use imp::{claim_app_id, focus_existing, open_url, serial_ports_key};
#[cfg(windows)]
pub(crate) use windows::process_running;

pub(crate) const MINIMIZED_FLAG: &str = "--minimized";

fn launch_command() -> Option<(PathBuf, String)> {
    let exe = std::env::current_exe().ok()?;
    let line = format!("\"{}\" {MINIMIZED_FLAG}", exe.display());
    Some((exe, line))
}

pub(crate) fn autostart_enabled() -> bool {
    imp::autostart_value().is_some()
}

pub(crate) fn set_autostart(on: bool) -> Result<(), String> {
    if on {
        let (_, line) = launch_command().ok_or("can't find the program path")?;
        imp::set_autostart(Some(&line))
    } else {
        imp::set_autostart(None)
    }
}

pub(crate) fn refresh_autostart() {
    let Some(stored) = imp::autostart_value() else {
        return;
    };
    if let Some((_, line)) = launch_command()
        && stored != line
    {
        match imp::set_autostart(Some(&line)) {
            Ok(()) => tracing::info!("autostart now points at this copy"),
            Err(error) => tracing::warn!(error = %error, "autostart refresh: {error}"),
        }
    }
}
