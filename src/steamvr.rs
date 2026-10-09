use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

pub const APP_KEY: &str = "dev.whyknot.hr-osc-rust";
pub const LAUNCH_FLAG: &str = "--steamvr";
const OVERLAY_KEY: &str = "dev.whyknot.hr-osc-rust.panel";
const MANIFEST: &str = "hr-osc-rust.vrmanifest";
const ICON: &str = "hr-osc-rust-icon.png";
const ICON_BYTES: &[u8] = include_bytes!("../assets/icon.png");

pub const SUPPORTED: bool = cfg!(windows);

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

pub fn runtime_dirs(vrpath: &str) -> Vec<PathBuf> {
    serde_json::from_str::<serde_json::Value>(vrpath)
        .ok()
        .and_then(|v| {
            v.get("runtime")?.as_array().map(|list| {
                list.iter()
                    .filter_map(|p| p.as_str().map(PathBuf::from))
                    .collect()
            })
        })
        .unwrap_or_default()
}

fn json_string(text: &str) -> String {
    serde_json::Value::String(text.to_owned()).to_string()
}

pub fn manifest(binary: &str, image: &str) -> String {
    format!(
        "{{\n  \"source\": \"builtin\",\n  \"applications\": [\n    {{\n      \"app_key\": {},\n      \"launch_type\": \"binary\",\n      \"binary_path_windows\": {},\n      \"arguments\": {},\n      \"is_dashboard_overlay\": true,\n      \"image_path\": {},\n      \"strings\": {{\n        \"en_us\": {{\n          \"name\": \"hr-osc-rust\",\n          \"description\": \"Heart rate to VRChat over OSC\"\n        }}\n      }}\n    }}\n  ]\n}}\n",
        json_string(APP_KEY),
        json_string(binary),
        json_string(LAUNCH_FLAG),
        json_string(image),
    )
}

fn write_files(data_dir: &Path) -> Result<(PathBuf, PathBuf), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let beside = exe.parent().map(Path::to_path_buf);
    let attempt = |dir: &Path, relative: bool| -> std::io::Result<(PathBuf, PathBuf)> {
        let icon = dir.join(ICON);
        std::fs::write(&icon, ICON_BYTES)?;
        let (binary, image) = if relative {
            (
                exe.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                ICON.to_owned(),
            )
        } else {
            (exe.display().to_string(), icon.display().to_string())
        };
        let path = dir.join(MANIFEST);
        std::fs::write(&path, manifest(&binary, &image))?;
        Ok((path, icon))
    };
    if let Some(dir) = beside
        && let Ok(found) = attempt(&dir, true)
    {
        return Ok(found);
    }
    attempt(data_dir, false).map_err(|e| e.to_string())
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

#[cfg(not(windows))]
pub fn running() -> bool {
    false
}

#[cfg(windows)]
mod ffi {
    use std::ffi::{CStr, CString, c_char, c_void};
    use std::path::Path;

    use windows_sys::Win32::Foundation::{FreeLibrary, HMODULE};
    use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

    pub const APP_OVERLAY: i32 = 2;
    pub const APP_BACKGROUND: i32 = 3;

    pub mod apps {
        pub const ADD_MANIFEST: usize = 0;
        pub const REMOVE_MANIFEST: usize = 1;
        pub const IS_INSTALLED: usize = 2;
        pub const IDENTIFY: usize = 11;
        pub const SET_AUTO_LAUNCH: usize = 17;
    }

    pub mod overlay {
        pub const DESTROY: usize = 2;
        pub const SET_WIDTH: usize = 21;
        pub const IS_VISIBLE: usize = 43;
        pub const POLL_EVENT: usize = 45;
        pub const SET_RAW: usize = 59;
        pub const SET_FROM_FILE: usize = 60;
        pub const CREATE_DASHBOARD: usize = 64;
    }

    pub mod system {
        pub const POLL_EVENT: usize = 29;
        pub const ACK_QUIT: usize = 43;
    }

    pub const EVENT_QUIT: u32 = 700;
    pub const EVENT_OVERLAY_SHOWN: u32 = 500;

    #[repr(C)]
    #[derive(Default)]
    pub struct Event {
        pub kind: u32,
        pub device: u32,
        pub age: f32,
        pub data: [u64; 6],
    }

    type InitFn = unsafe extern "C" fn(*mut i32, i32, *const c_char) -> u32;
    type ShutdownFn = unsafe extern "C" fn();
    type InterfaceFn = unsafe extern "C" fn(*const c_char, *mut i32) -> isize;
    type SymbolFn = unsafe extern "C" fn(i32) -> *const c_char;

    pub struct OpenVr {
        module: HMODULE,
        init: InitFn,
        shutdown: ShutdownFn,
        interface: InterfaceFn,
        symbol: SymbolFn,
        active: bool,
    }

    pub struct Table(*const usize);

    impl Table {
        pub unsafe fn get<F: Copy>(&self, index: usize) -> F {
            unsafe {
                let raw = *self.0.add(index);
                std::mem::transmute_copy(&raw)
            }
        }
    }

    pub fn cstr(text: &str) -> CString {
        CString::new(text).unwrap_or_default()
    }

    impl OpenVr {
        pub fn load(runtime: &Path) -> Result<OpenVr, String> {
            let dll = runtime.join("bin").join("win64").join("openvr_api.dll");
            let wide: Vec<u16> = dll.as_os_str().encode_wide_null();
            let module = unsafe { LoadLibraryW(wide.as_ptr()) };
            if module.is_null() {
                return Err(format!("can't load {}", dll.display()));
            }
            let lookup = |name: &[u8]| unsafe { GetProcAddress(module, name.as_ptr()) };
            let (Some(init), Some(shutdown), Some(interface), Some(symbol)) = (
                lookup(b"VR_InitInternal2\0"),
                lookup(b"VR_ShutdownInternal\0"),
                lookup(b"VR_GetGenericInterface\0"),
                lookup(b"VR_GetVRInitErrorAsSymbol\0"),
            ) else {
                unsafe { FreeLibrary(module) };
                return Err("openvr_api.dll is missing exports".into());
            };
            unsafe {
                Ok(OpenVr {
                    module,
                    init: std::mem::transmute::<unsafe extern "system" fn() -> isize, InitFn>(init),
                    shutdown: std::mem::transmute::<unsafe extern "system" fn() -> isize, ShutdownFn>(
                        shutdown,
                    ),
                    interface: std::mem::transmute::<
                        unsafe extern "system" fn() -> isize,
                        InterfaceFn,
                    >(interface),
                    symbol: std::mem::transmute::<unsafe extern "system" fn() -> isize, SymbolFn>(
                        symbol,
                    ),
                    active: false,
                })
            }
        }

        pub fn start(&mut self, kind: i32) -> Result<(), String> {
            let mut error = 0i32;
            unsafe { (self.init)(&mut error, kind, std::ptr::null()) };
            if error != 0 {
                return Err(self.describe(error));
            }
            self.active = true;
            Ok(())
        }

        fn describe(&self, error: i32) -> String {
            let text = unsafe { (self.symbol)(error) };
            if text.is_null() {
                return format!("error {error}");
            }
            unsafe { CStr::from_ptr(text) }
                .to_string_lossy()
                .into_owned()
        }

        pub fn table(&self, version: &str) -> Result<Table, String> {
            let name = cstr(&format!("FnTable:{version}"));
            let mut error = 0i32;
            let raw = unsafe { (self.interface)(name.as_ptr(), &mut error) };
            if error != 0 || raw == 0 {
                return Err(format!("{version}: {}", self.describe(error)));
            }
            Ok(Table(raw as *const usize))
        }
    }

    impl Drop for OpenVr {
        fn drop(&mut self) {
            unsafe {
                if self.active {
                    (self.shutdown)();
                }
                FreeLibrary(self.module);
            }
        }
    }

    trait EncodeWideNull {
        fn encode_wide_null(&self) -> Vec<u16>;
    }

    impl EncodeWideNull for std::ffi::OsStr {
        fn encode_wide_null(&self) -> Vec<u16> {
            use std::os::windows::ffi::OsStrExt;
            self.encode_wide().chain(Some(0)).collect()
        }
    }

    pub type AddManifestFn = unsafe extern "system" fn(*const c_char, bool) -> i32;
    pub type RemoveManifestFn = unsafe extern "system" fn(*const c_char) -> i32;
    pub type IsInstalledFn = unsafe extern "system" fn(*const c_char) -> bool;
    pub type IdentifyFn = unsafe extern "system" fn(u32, *const c_char) -> i32;
    pub type SetAutoLaunchFn = unsafe extern "system" fn(*const c_char, bool) -> i32;
    pub type DestroyFn = unsafe extern "system" fn(u64) -> i32;
    pub type SetWidthFn = unsafe extern "system" fn(u64, f32) -> i32;
    pub type IsVisibleFn = unsafe extern "system" fn(u64) -> bool;
    pub type PollOverlayFn = unsafe extern "system" fn(u64, *mut Event, u32) -> bool;
    pub type SetRawFn = unsafe extern "system" fn(u64, *mut c_void, u32, u32, u32) -> i32;
    pub type SetFromFileFn = unsafe extern "system" fn(u64, *const c_char) -> i32;
    pub type CreateDashboardFn =
        unsafe extern "system" fn(*const c_char, *const c_char, *mut u64, *mut u64) -> i32;
    pub type PollSystemFn = unsafe extern "system" fn(*mut Event, u32) -> bool;
    pub type AckQuitFn = unsafe extern "system" fn();
}

#[cfg(windows)]
fn load_runtime() -> Result<ffi::OpenVr, String> {
    let base = std::env::var_os("LOCALAPPDATA").ok_or("no LOCALAPPDATA")?;
    let vrpath = std::fs::read_to_string(
        PathBuf::from(base)
            .join("openvr")
            .join("openvrpaths.vrpath"),
    )
    .map_err(|_| "SteamVR isn't installed".to_owned())?;
    let mut last = String::from("SteamVR isn't installed");
    for dir in runtime_dirs(&vrpath) {
        match ffi::OpenVr::load(&dir) {
            Ok(vr) => return Ok(vr),
            Err(error) => last = error,
        }
    }
    Err(last)
}

#[cfg(windows)]
fn apply_registration(vr: &ffi::OpenVr, enable: bool, data_dir: &Path) -> Result<(), String> {
    let apps = vr.table("IVRApplications_007")?;
    let key = ffi::cstr(APP_KEY);
    unsafe {
        let set_auto: ffi::SetAutoLaunchFn = apps.get(ffi::apps::SET_AUTO_LAUNCH);
        if enable {
            let (manifest, _) = write_files(data_dir)?;
            let path = ffi::cstr(&manifest.display().to_string());
            let add: ffi::AddManifestFn = apps.get(ffi::apps::ADD_MANIFEST);
            let error = add(path.as_ptr(), false);
            if error != 0 {
                return Err(format!("adding the manifest failed ({error})"));
            }
            let error = set_auto(key.as_ptr(), true);
            if error != 0 {
                return Err(format!("turning on auto launch failed ({error})"));
            }
        } else {
            let installed: ffi::IsInstalledFn = apps.get(ffi::apps::IS_INSTALLED);
            if installed(key.as_ptr()) {
                set_auto(key.as_ptr(), false);
            }
            let remove: ffi::RemoveManifestFn = apps.get(ffi::apps::REMOVE_MANIFEST);
            for dir in [
                std::env::current_exe()
                    .ok()
                    .and_then(|e| e.parent().map(Path::to_path_buf)),
                Some(data_dir.to_path_buf()),
            ]
            .into_iter()
            .flatten()
            {
                let path = dir.join(MANIFEST);
                if path.is_file() {
                    remove(ffi::cstr(&path.display().to_string()).as_ptr());
                }
            }
        }
    }
    Ok(())
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
    let thread = std::thread::Builder::new()
        .name("steamvr".into())
        .spawn(move || supervise(options, rx, on_registered, on_quit))
        .ok();
    Handle { tx, thread }
}

#[derive(Clone, Default, PartialEq)]
pub(crate) struct PanelView {
    pub connected: bool,
    pub bpm: u16,
    pub percent: String,
    pub status: String,
}

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

#[cfg(windows)]
enum SessionEnd {
    SteamVrQuit,
    AppQuit,
    Unregistered,
}

#[cfg(windows)]
fn session(
    enabled: &mut bool,
    registered: &mut bool,
    on_registered: &dyn Fn(bool),
    data_dir: &Path,
    rx: &Receiver<Msg>,
    view: &mut PanelView,
) -> Result<SessionEnd, String> {
    let mut note = |state: bool| {
        if *registered != state {
            *registered = state;
            on_registered(state);
        }
    };
    let mut vr = load_runtime()?;
    if !*enabled {
        vr.start(ffi::APP_BACKGROUND)?;
        apply_registration(&vr, false, data_dir)?;
        crate::log::write("steamvr: removed from SteamVR start-up");
        note(false);
        return Ok(SessionEnd::Unregistered);
    }
    vr.start(ffi::APP_OVERLAY)?;
    apply_registration(&vr, true, data_dir)?;
    note(true);
    crate::log::write("steamvr: registered and connected");
    let (_, icon) = write_files(data_dir)?;
    let apps = vr.table("IVRApplications_007")?;
    let overlay = vr.table("IVROverlay_025")?;
    let system = vr.table("IVRSystem_022")?;
    let key = ffi::cstr(APP_KEY);
    let mut panel = crate::vr_panel::Panel::new()?;
    unsafe {
        let identify: ffi::IdentifyFn = apps.get(ffi::apps::IDENTIFY);
        identify(std::process::id(), key.as_ptr());
        let create: ffi::CreateDashboardFn = overlay.get(ffi::overlay::CREATE_DASHBOARD);
        let (mut main, mut thumb) = (0u64, 0u64);
        let error = create(
            ffi::cstr(OVERLAY_KEY).as_ptr(),
            ffi::cstr("hr-osc-rust").as_ptr(),
            &mut main,
            &mut thumb,
        );
        if error != 0 {
            return Err(format!("creating the dashboard overlay failed ({error})"));
        }
        let from_file: ffi::SetFromFileFn = overlay.get(ffi::overlay::SET_FROM_FILE);
        from_file(thumb, ffi::cstr(&icon.display().to_string()).as_ptr());
        let set_width: ffi::SetWidthFn = overlay.get(ffi::overlay::SET_WIDTH);
        set_width(main, 1.5);
        let set_raw: ffi::SetRawFn = overlay.get(ffi::overlay::SET_RAW);
        let is_visible: ffi::IsVisibleFn = overlay.get(ffi::overlay::IS_VISIBLE);
        let poll_overlay: ffi::PollOverlayFn = overlay.get(ffi::overlay::POLL_EVENT);
        let poll_system: ffi::PollSystemFn = system.get(ffi::system::POLL_EVENT);
        let ack_quit: ffi::AckQuitFn = system.get(ffi::system::ACK_QUIT);
        let destroy: ffi::DestroyFn = overlay.get(ffi::overlay::DESTROY);
        let size = std::mem::size_of::<ffi::Event>() as u32;
        let mut stale = true;
        let end = loop {
            let mut event = ffi::Event::default();
            let mut quit = false;
            while poll_system(&mut event, size) {
                quit |= event.kind == ffi::EVENT_QUIT;
            }
            while poll_overlay(main, &mut event, size) {
                stale |= event.kind == ffi::EVENT_OVERLAY_SHOWN;
            }
            if quit {
                ack_quit();
                crate::log::write("steamvr: SteamVR is closing");
                break SessionEnd::SteamVrQuit;
            }
            if stale && (is_visible(main) || !panel.drawn()) {
                let (pixels, width, height) = panel.render(view);
                set_raw(main, pixels.as_ptr() as *mut _, width, height, 4);
                stale = false;
            }
            match rx.recv_timeout(Duration::from_millis(500)) {
                Ok(Msg::Quit) | Err(RecvTimeoutError::Disconnected) => break SessionEnd::AppQuit,
                Ok(Msg::Enable(on)) => {
                    *enabled = on;
                    if !on {
                        destroy(main);
                        apply_registration(&vr, false, data_dir)?;
                        crate::log::write("steamvr: removed from SteamVR start-up");
                        note(false);
                        return Ok(SessionEnd::Unregistered);
                    }
                }
                Ok(Msg::View {
                    connected,
                    bpm,
                    percent,
                    status,
                }) => {
                    let next = PanelView {
                        connected,
                        bpm,
                        percent,
                        status,
                    };
                    stale |= next != *view;
                    *view = next;
                }
                Err(RecvTimeoutError::Timeout) => {}
            }
        };
        destroy(main);
        Ok(end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_runtime_paths() {
        let text = r#"{"config":["C:\\Steam\\config"],"runtime":["C:\\Program Files (x86)\\Steam\\steamapps\\common\\SteamVR","D:\\SteamVR"],"version":1}"#;
        assert_eq!(
            runtime_dirs(text),
            vec![
                PathBuf::from(r"C:\Program Files (x86)\Steam\steamapps\common\SteamVR"),
                PathBuf::from(r"D:\SteamVR")
            ]
        );
        assert!(runtime_dirs("{}").is_empty());
        assert!(runtime_dirs("garbage").is_empty());
    }

    #[test]
    fn manifest_is_valid_json_with_the_overlay_flag() {
        let text = manifest(r#"C:\it's "here"\hr-osc-rust.exe"#, "hr-osc-rust-icon.png");
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        let app = &value["applications"][0];
        assert_eq!(app["app_key"], APP_KEY);
        assert_eq!(app["is_dashboard_overlay"], true);
        assert_eq!(app["launch_type"], "binary");
        assert_eq!(app["arguments"], LAUNCH_FLAG);
        assert_eq!(
            app["binary_path_windows"],
            r#"C:\it's "here"\hr-osc-rust.exe"#
        );
        assert_eq!(app["image_path"], "hr-osc-rust-icon.png");
        assert_eq!(value["source"], "builtin");
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "needs SteamVR installed and closed"]
    fn background_init_leaves_steamvr_closed() {
        assert!(!running(), "close SteamVR first");
        let mut vr = load_runtime().expect("SteamVR's openvr_api.dll loads");
        let error = vr
            .start(ffi::APP_BACKGROUND)
            .expect_err("SteamVR isn't running");
        assert!(error.contains("NoServerForBackgroundApp"), "{error}");
        drop(vr);
        assert!(!running(), "the probe must not start SteamVR");
    }

    #[cfg(windows)]
    #[test]
    fn event_matches_the_sdk_layout() {
        assert_eq!(std::mem::size_of::<ffi::Event>(), 64);
        assert_eq!(std::mem::offset_of!(ffi::Event, data), 16);
    }
}
