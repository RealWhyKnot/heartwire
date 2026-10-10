#[cfg(windows)]
mod dashboard;
#[cfg(windows)]
mod openvr;
#[cfg(any(windows, test))]
pub(crate) mod panel;
#[cfg(windows)]
mod registration;
mod supervisor;

use std::path::PathBuf;
use std::sync::mpsc::Sender;

#[cfg(windows)]
pub const APP_KEY: &str = "dev.whyknot.heartwire";

pub const LAUNCH_FLAG: &str = "--steamvr";

pub const REGISTER_FLAG: &str = "--register-steamvr";

pub const UNREGISTER_FLAG: &str = "--unregister-steamvr";

pub const SUPPORTED: bool = cfg!(windows);

pub fn setup_command(register: bool, data_dir: &std::path::Path) -> i32 {
    #[cfg(windows)]
    {
        if register {
            registration::register_for_setup(data_dir)
        } else {
            registration::unregister_for_setup(data_dir)
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (register, data_dir);
        0
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PanelView {
    pub connected: bool,
    pub bpm: u16,
    pub percent: String,
    pub status: String,
}

#[cfg_attr(not(windows), allow(dead_code))]
pub enum Msg {
    Enable(bool),
    View(PanelView),
    Quit,
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

#[cfg(windows)]
struct SteamVr<R: Fn(bool)> {
    data_dir: PathBuf,
    on_registered: R,
}

#[cfg(windows)]
impl<R: Fn(bool)> supervisor::Runtime for SteamVr<R> {
    fn running(&mut self) -> bool {
        crate::platform::process_running("vrserver.exe")
    }

    fn session(
        &mut self,
        enabled: &mut bool,
        registered: &mut bool,
        rx: &std::sync::mpsc::Receiver<Msg>,
        view: &mut PanelView,
    ) -> Result<supervisor::SessionEnd, String> {
        dashboard::session(
            enabled,
            registered,
            &self.on_registered,
            &self.data_dir,
            rx,
            view,
        )
    }
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
        .spawn(move || {
            let mut runtime = SteamVr {
                data_dir: options.data_dir.clone(),
                on_registered,
            };
            supervisor::supervise(&options, &rx, &mut runtime, on_quit);
        })
        .ok();
    #[cfg(not(windows))]
    let thread = {
        drop((options, rx, on_registered, on_quit));
        None
    };
    Handle { tx, thread }
}

#[cfg(all(test, windows))]
mod tests {
    #[test]
    #[ignore = "needs SteamVR running"]
    fn sees_a_running_steamvr() {
        assert!(crate::platform::process_running("vrserver.exe"));
    }
}
