use std::path::PathBuf;

pub const MINIMIZED_FLAG: &str = "--minimized";

fn launch_command() -> Option<(PathBuf, String)> {
    let exe = std::env::current_exe().ok()?;
    let line = format!("\"{}\" {MINIMIZED_FLAG}", exe.display());
    Some((exe, line))
}

#[cfg(windows)]
mod imp {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::null;

    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
    };
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        FindWindowW, SW_RESTORE, SW_SHOWNORMAL, SetForegroundWindow, ShowWindow,
    };

    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const VALUE: &str = "hr-osc-rust";

    fn wide(text: &str) -> Vec<u16> {
        OsStr::new(text).encode_wide().chain(Some(0)).collect()
    }

    pub fn autostart_value() -> Option<String> {
        let key = wide(RUN_KEY);
        let name = wide(VALUE);
        let mut bytes: u32 = 0;
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut bytes,
            )
        };
        if status != ERROR_SUCCESS || bytes < 2 {
            return None;
        }
        let mut buf = vec![0u16; bytes as usize / 2];
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                buf.as_mut_ptr().cast(),
                &mut bytes,
            )
        };
        if status != ERROR_SUCCESS {
            return None;
        }
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..end]))
    }

    pub fn set_autostart(command: Option<&str>) -> Result<(), String> {
        let key = wide(RUN_KEY);
        let name = wide(VALUE);
        let status = match command {
            Some(command) => {
                let data = wide(command);
                unsafe {
                    RegSetKeyValueW(
                        HKEY_CURRENT_USER,
                        key.as_ptr(),
                        name.as_ptr(),
                        REG_SZ,
                        data.as_ptr().cast(),
                        (data.len() * 2) as u32,
                    )
                }
            }
            None => unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr()) },
        };
        if status == ERROR_SUCCESS || (command.is_none() && status == 2) {
            Ok(())
        } else {
            Err(format!("registry error {status}"))
        }
    }

    pub fn open_url(url: &str) {
        let verb = wide("open");
        let target = wide(url);
        unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                target.as_ptr(),
                null(),
                null(),
                SW_SHOWNORMAL,
            );
        }
    }

    pub fn focus_existing(title: &str) -> bool {
        let title = wide(title);
        unsafe {
            let window = FindWindowW(null(), title.as_ptr());
            if window.is_null() {
                return false;
            }
            ShowWindow(window, SW_RESTORE);
            SetForegroundWindow(window);
        }
        true
    }
}

#[cfg(not(windows))]
mod imp {
    use std::path::PathBuf;

    fn home() -> PathBuf {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default()
    }

    #[cfg(target_os = "macos")]
    fn entry() -> PathBuf {
        home().join("Library/LaunchAgents/dev.whyknot.hr-osc-rust.plist")
    }

    #[cfg(not(target_os = "macos"))]
    fn entry() -> PathBuf {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join(".config"))
            .join("autostart/hr-osc-rust.desktop")
    }

    #[cfg(target_os = "macos")]
    fn render(command: &str) -> String {
        let exe = super::launch_command()
            .map(|c| c.0.display().to_string())
            .unwrap_or_default();
        let _ = command;
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n  <key>Label</key>\n  <string>dev.whyknot.hr-osc-rust</string>\n  <key>ProgramArguments</key>\n  <array>\n    <string>{exe}</string>\n    <string>{}</string>\n  </array>\n  <key>RunAtLoad</key>\n  <true/>\n</dict>\n</plist>\n",
            super::MINIMIZED_FLAG
        )
    }

    #[cfg(not(target_os = "macos"))]
    fn render(command: &str) -> String {
        format!(
            "[Desktop Entry]\nType=Application\nName=hr-osc-rust\nExec={command}\nX-GNOME-Autostart-enabled=true\n"
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
}

pub use imp::{focus_existing, open_url};

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
