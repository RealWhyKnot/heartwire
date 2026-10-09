use std::path::PathBuf;

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
}

#[cfg(target_os = "macos")]
fn entry() -> PathBuf {
    home().join("Library/LaunchAgents/dev.whyknot.heartwire.plist")
}

#[cfg(not(target_os = "macos"))]
fn entry() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".config"))
        .join("autostart/heartwire.desktop")
}

#[cfg(target_os = "macos")]
fn render(command: &str) -> String {
    let exe = super::launch_command()
        .map(|c| c.0.display().to_string())
        .unwrap_or_default();
    let _ = command;
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n  <key>Label</key>\n  <string>dev.whyknot.heartwire</string>\n  <key>ProgramArguments</key>\n  <array>\n    <string>{exe}</string>\n    <string>{}</string>\n  </array>\n  <key>RunAtLoad</key>\n  <true/>\n</dict>\n</plist>\n",
        super::MINIMIZED_FLAG
    )
}

#[cfg(not(target_os = "macos"))]
fn render(command: &str) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName=Heartwire\nExec={command}\nX-GNOME-Autostart-enabled=true\n"
    )
}

pub fn autostart_value() -> Option<String> {
    let text = std::fs::read_to_string(entry()).ok()?;
    let current = super::launch_command()?.1;
    Some(if text == render(&current) {
        current
    } else {
        text
    })
}

pub fn set_autostart(command: Option<&str>) -> Result<(), String> {
    let path = entry();
    match command {
        Some(command) => {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            std::fs::write(&path, render(command)).map_err(|e| e.to_string())
        }
        None => match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        },
    }
}

pub fn serial_ports_key() -> Option<Vec<u16>> {
    None
}

pub fn open_url(url: &str) {
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(opener).arg(url).spawn();
}

pub fn focus_existing(_title: &str) -> bool {
    false
}
