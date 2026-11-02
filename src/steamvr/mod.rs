#[cfg(windows)]
mod dashboard;
#[cfg(windows)]
mod openvr;
#[cfg(any(windows, test))]
pub(crate) mod panel;
#[cfg(windows)]
mod registration;
#[cfg(any(windows, test))]
mod supervisor;

#[cfg(windows)]
use std::path::PathBuf;

#[cfg(windows)]
pub(crate) const APP_KEY: &str = "dev.whyknot.heartwire";

pub(crate) const LAUNCH_FLAG: &str = "--steamvr";

pub(crate) const REGISTER_FLAG: &str = "--register-steamvr";

pub(crate) const UNREGISTER_FLAG: &str = "--unregister-steamvr";

pub(crate) const SUPPORTED: bool = cfg!(windows);

pub(crate) fn setup_command(register: bool, data_dir: &std::path::Path) -> i32 {
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
pub(crate) struct PanelView {
    pub connected: bool,
    pub bpm: u16,
    pub percent: String,
    pub status: String,
}

#[cfg(any(windows, test))]
pub(crate) enum Msg {
    Enable(bool),
    View(PanelView),
    Quit,
}

#[derive(Clone)]
pub(crate) struct Sender {
    #[cfg(windows)]
    inner: std::sync::mpsc::Sender<Msg>,
}

impl Sender {
    pub(crate) fn enable(&self, enabled: bool) {
        #[cfg(windows)]
        let _ = self.inner.send(Msg::Enable(enabled));
        #[cfg(not(windows))]
        let _ = enabled;
    }

    pub(crate) fn view(&self, view: PanelView) {
        #[cfg(windows)]
        let _ = self.inner.send(Msg::View(view));
        #[cfg(not(windows))]
        drop(view);
    }
}

pub(crate) struct Handle {
    tx: Sender,
    #[cfg(windows)]
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Handle {
    pub(crate) fn sender(&self) -> Sender {
        self.tx.clone()
    }
}

#[cfg(windows)]
impl Drop for Handle {
    fn drop(&mut self) {
        let _ = self.tx.inner.send(Msg::Quit);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

pub(crate) struct Options {
    #[cfg(any(windows, test))]
    pub enabled: bool,
    #[cfg(any(windows, test))]
    pub registered: bool,
    #[cfg(windows)]
    pub data_dir: PathBuf,
    #[cfg(any(windows, test))]
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

pub(crate) fn start(
    options: Options,
    on_registered: impl Fn(bool) + Send + 'static,
    on_quit: impl Fn() + Send + 'static,
) -> Handle {
    #[cfg(windows)]
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
    let _ = (options, on_registered, on_quit);
    Handle {
        tx: Sender {
            #[cfg(windows)]
            inner: tx,
        },
        #[cfg(windows)]
        thread,
    }
}

#[cfg(all(test, windows))]
mod tests {
    #[test]
    #[ignore = "needs SteamVR running"]
    fn sees_a_running_steamvr() {
        assert!(crate::platform::process_running("vrserver.exe"));
    }
}
