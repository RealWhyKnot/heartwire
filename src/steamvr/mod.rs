#[cfg(windows)]
mod dashboard;
#[cfg(windows)]
mod openvr;
#[cfg(any(windows, test))]
pub(crate) mod panel;
#[cfg(windows)]
mod registration;

use std::path::PathBuf;
use std::sync::mpsc::Sender;
#[cfg(windows)]
use std::sync::mpsc::{Receiver, RecvTimeoutError};
#[cfg(windows)]
use std::time::{Duration, Instant};

#[cfg(windows)]
use dashboard::{SessionEnd, session};

#[cfg(windows)]
pub const APP_KEY: &str = "dev.whyknot.heartwire";

pub const LAUNCH_FLAG: &str = "--steamvr";

pub const SUPPORTED: bool = cfg!(windows);

#[cfg_attr(not(windows), allow(dead_code))]
pub enum Msg {
    Enable(bool),
    View {
        connected: bool,
        bpm: u16,
        percent: String,
        status: String,
    },
    Quit,
}

#[cfg(windows)]
pub fn running() -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return false;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut found = false;
        let mut more = Process32FirstW(snapshot, &mut entry) != 0;
        while more {
            let end = entry
                .szExeFile
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(entry.szExeFile.len());
            if String::from_utf16_lossy(&entry.szExeFile[..end])
                .eq_ignore_ascii_case("vrserver.exe")
            {
                found = true;
                break;
            }
            more = Process32NextW(snapshot, &mut entry) != 0;
        }
        CloseHandle(snapshot);
        found
    }
}

pub struct Handle {
    tx: Sender<Msg>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Handle {
    pub fn sender(&self) -> Sender<Msg> {
        self.tx.clone()
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        let _ = self.tx.send(Msg::Quit);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
pub struct Options {
    pub enabled: bool,
    pub registered: bool,
    pub data_dir: PathBuf,
    pub launched: bool,
}

pub fn start(
    options: Options,
    on_registered: impl Fn(bool) + Send + 'static,
    on_quit: impl Fn() + Send + 'static,
) -> Handle {
    let (tx, rx) = std::sync::mpsc::channel();
    #[cfg(windows)]
    let thread = std::thread::Builder::new()
        .name("steamvr".into())
        .spawn(move || supervise(options, rx, on_registered, on_quit))
        .ok();
    #[cfg(not(windows))]
    let thread = {
        drop((options, rx, on_registered, on_quit));
        None
    };
    Handle { tx, thread }
}

#[derive(Clone, Default, PartialEq)]
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) struct PanelView {
    pub connected: bool,
    pub bpm: u16,
    pub percent: String,
    pub status: String,
}

#[cfg(windows)]
fn supervise(
    options: Options,
    rx: Receiver<Msg>,
    on_registered: impl Fn(bool),
    on_quit: impl Fn(),
) {
    let Options {
        mut enabled,
        mut registered,
        data_dir,
        launched,
    } = options;
    let mut view = PanelView::default();
    let mut cooldown = Instant::now();
    loop {
        if (enabled || registered) && Instant::now() >= cooldown && running() {
            #[cfg(windows)]
            match session(
                &mut enabled,
                &mut registered,
                &on_registered,
                &data_dir,
                &rx,
                &mut view,
            ) {
                Ok(SessionEnd::SteamVrQuit) => {
                    if launched {
                        on_quit();
                        return;
                    }
                    cooldown = Instant::now() + Duration::from_secs(20);
                }
                Ok(SessionEnd::AppQuit) => return,
                Ok(SessionEnd::Unregistered) => {}
                Err(error) => {
                    crate::log::write(&format!("steamvr: {error}"));
                    cooldown = Instant::now() + Duration::from_secs(30);
                }
            }
        }
        let wait = if enabled || registered {
            Duration::from_secs(10)
        } else {
            Duration::from_secs(3600)
        };
        match rx.recv_timeout(wait) {
            Ok(Msg::Quit) | Err(RecvTimeoutError::Disconnected) => return,
            Ok(Msg::Enable(on)) => enabled = on,
            Ok(Msg::View {
                connected,
                bpm,
                percent,
                status,
            }) => {
                view = PanelView {
                    connected,
                    bpm,
                    percent,
                    status,
                };
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
    }
}
