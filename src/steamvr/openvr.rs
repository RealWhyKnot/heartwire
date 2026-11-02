use std::ffi::{CStr, CString, c_char, c_void};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};

use windows_sys::Win32::Foundation::{FreeLibrary, HMODULE};
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

pub(super) const APP_OVERLAY: i32 = 2;
#[cfg(test)]
pub(super) const APP_BACKGROUND: i32 = 3;
pub(super) const APP_UTILITY: i32 = 4;

mod apps {
    pub(super) const ADD_MANIFEST: usize = 0;
    pub(super) const REMOVE_MANIFEST: usize = 1;
    pub(super) const IS_INSTALLED: usize = 2;
    pub(super) const IDENTIFY: usize = 11;
    pub(super) const SET_AUTO_LAUNCH: usize = 17;
}

mod overlay {
    pub(super) const DESTROY: usize = 2;
    pub(super) const SET_WIDTH: usize = 21;
    pub(super) const IS_VISIBLE: usize = 43;
    pub(super) const POLL_EVENT: usize = 45;
    pub(super) const SET_RAW: usize = 59;
    pub(super) const SET_FROM_FILE: usize = 60;
    pub(super) const CREATE_DASHBOARD: usize = 64;
}

mod system {
    pub(super) const POLL_EVENT: usize = 29;
    pub(super) const ACK_QUIT: usize = 43;
}

pub(super) const EVENT_QUIT: u32 = 700;
pub(super) const EVENT_OVERLAY_SHOWN: u32 = 500;

#[repr(C)]
#[derive(Default)]
pub(super) struct Event {
    pub kind: u32,
    pub device: u32,
    pub age: f32,
    pub data: [u64; 6],
}

type InitFn = unsafe extern "C" fn(*mut i32, i32, *const c_char) -> u32;
type ShutdownFn = unsafe extern "C" fn();
type InterfaceFn = unsafe extern "C" fn(*const c_char, *mut i32) -> isize;
type SymbolFn = unsafe extern "C" fn(i32) -> *const c_char;

pub(super) struct OpenVr {
    module: HMODULE,
    init: InitFn,
    shutdown: ShutdownFn,
    interface: InterfaceFn,
    symbol: SymbolFn,
    active: bool,
}

#[derive(Clone, Copy)]
struct Table(*const usize);

impl Table {
    unsafe fn get<F: Copy>(&self, index: usize) -> F {
        // SAFETY: the caller supplies an SDK slot and its matching pointer type in a live function table.
        unsafe {
            let raw = *self.0.add(index);
            std::mem::transmute_copy(&raw)
        }
    }
}

fn cstr(text: &str) -> CString {
    CString::new(text).unwrap_or_default()
}

impl OpenVr {
    pub(super) fn load(runtime: &Path) -> Result<OpenVr, String> {
        let dll = runtime.join("bin").join("win64").join("openvr_api.dll");
        let wide: Vec<u16> = dll.as_os_str().encode_wide_null();
        // SAFETY: wide is a terminated UTF-16 path that remains live for the call.
        let module = unsafe { LoadLibraryW(wide.as_ptr()) };
        if module.is_null() {
            return Err(format!("can't load {}", dll.display()));
        }
        // SAFETY: module is loaded and each lookup below supplies a terminated export name.
        let lookup = |name: &[u8]| unsafe { GetProcAddress(module, name.as_ptr()) };
        let (Some(init), Some(shutdown), Some(interface), Some(symbol)) = (
            lookup(b"VR_InitInternal2\0"),
            lookup(b"VR_ShutdownInternal\0"),
            lookup(b"VR_GetGenericInterface\0"),
            lookup(b"VR_GetVRInitErrorAsSymbol\0"),
        ) else {
            // SAFETY: module was loaded successfully and no exported pointer is used after this release.
            unsafe { FreeLibrary(module) };
            return Err("openvr_api.dll is missing exports".into());
        };
        // SAFETY: the named OpenVR exports have these SDK signatures and module remains owned by OpenVr.
        unsafe {
            Ok(OpenVr {
                module,
                init: std::mem::transmute::<unsafe extern "system" fn() -> isize, InitFn>(init),
                shutdown: std::mem::transmute::<unsafe extern "system" fn() -> isize, ShutdownFn>(
                    shutdown,
                ),
                interface: std::mem::transmute::<unsafe extern "system" fn() -> isize, InterfaceFn>(
                    interface,
                ),
                symbol: std::mem::transmute::<unsafe extern "system" fn() -> isize, SymbolFn>(
                    symbol,
                ),
                active: false,
            })
        }
    }

    pub(super) fn start(&mut self, kind: i32) -> Result<(), String> {
        let mut error = 0i32;
        // SAFETY: init belongs to the loaded module; error is writable and the optional startup string is null.
        unsafe { (self.init)(&mut error, kind, std::ptr::null()) };
        if error != 0 {
            return Err(self.describe(error));
        }
        self.active = true;
        Ok(())
    }

    fn describe(&self, error: i32) -> String {
        // SAFETY: symbol is an SDK error lookup from the live module and accepts an integer error code.
        let text = unsafe { (self.symbol)(error) };
        if text.is_null() {
            return format!("error {error}");
        }
        // SAFETY: a nonnull SDK error symbol points to a terminated string owned by the loaded module.
        unsafe { CStr::from_ptr(text) }
            .to_string_lossy()
            .into_owned()
    }

    fn table(&self, version: &str) -> Result<Table, String> {
        let name = cstr(&format!("FnTable:{version}"));
        let mut error = 0i32;
        // SAFETY: name is a live terminated string and error is a writable SDK error output.
        let raw = unsafe { (self.interface)(name.as_ptr(), &mut error) };
        if error != 0 || raw == 0 {
            return Err(format!("{version}: {}", self.describe(error)));
        }
        Ok(Table(std::ptr::with_exposed_provenance(
            raw.cast_unsigned(),
        )))
    }
}

impl Drop for OpenVr {
    fn drop(&mut self) {
        // SAFETY: this owner retains the module and ends its active session before releasing it.
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

type AddManifestFn = unsafe extern "system" fn(*const c_char, bool) -> i32;
type RemoveManifestFn = unsafe extern "system" fn(*const c_char) -> i32;
type IsInstalledFn = unsafe extern "system" fn(*const c_char) -> bool;
type IdentifyFn = unsafe extern "system" fn(u32, *const c_char) -> i32;
type SetAutoLaunchFn = unsafe extern "system" fn(*const c_char, bool) -> i32;
type DestroyFn = unsafe extern "system" fn(u64) -> i32;
type SetWidthFn = unsafe extern "system" fn(u64, f32) -> i32;
type IsVisibleFn = unsafe extern "system" fn(u64) -> bool;
type PollOverlayFn = unsafe extern "system" fn(u64, *mut Event, u32) -> bool;
type SetRawFn = unsafe extern "system" fn(u64, *mut c_void, u32, u32, u32) -> i32;
type SetFromFileFn = unsafe extern "system" fn(u64, *const c_char) -> i32;
type CreateDashboardFn =
    unsafe extern "system" fn(*const c_char, *const c_char, *mut u64, *mut u64) -> i32;
type PollSystemFn = unsafe extern "system" fn(*mut Event, u32) -> bool;
type AckQuitFn = unsafe extern "system" fn();

pub(super) type OverlayHandle = u64;

pub(super) struct Applications<'a> {
    table: Table,
    _vr: PhantomData<&'a OpenVr>,
}

pub(super) struct Overlay<'a> {
    table: Table,
    _vr: PhantomData<&'a OpenVr>,
}

pub(super) struct System<'a> {
    table: Table,
    _vr: PhantomData<&'a OpenVr>,
}

impl OpenVr {
    pub(super) fn applications(&self) -> Result<Applications<'_>, String> {
        Ok(Applications {
            table: self.table("IVRApplications_007")?,
            _vr: PhantomData,
        })
    }

    pub(super) fn overlay(&self) -> Result<Overlay<'_>, String> {
        Ok(Overlay {
            table: self.table("IVROverlay_025")?,
            _vr: PhantomData,
        })
    }

    pub(super) fn system(&self) -> Result<System<'_>, String> {
        Ok(System {
            table: self.table("IVRSystem_022")?,
            _vr: PhantomData,
        })
    }
}

fn path_cstr(path: &Path) -> CString {
    cstr(&path.display().to_string())
}

impl Applications<'_> {
    pub(super) fn add_manifest(&self, path: &Path) -> i32 {
        let path = path_cstr(path);
        // SAFETY: this live applications table uses the SDK AddApplicationManifest slot and a terminated path.
        unsafe { self.table.get::<AddManifestFn>(apps::ADD_MANIFEST)(path.as_ptr(), false) }
    }

    pub(super) fn remove_manifest(&self, path: &Path) -> i32 {
        let path = path_cstr(path);
        // SAFETY: this live applications table uses the SDK RemoveApplicationManifest slot and a terminated path.
        unsafe { self.table.get::<RemoveManifestFn>(apps::REMOVE_MANIFEST)(path.as_ptr()) }
    }

    pub(super) fn is_installed(&self, key: &str) -> bool {
        let key = cstr(key);
        // SAFETY: this live applications table uses the SDK IsApplicationInstalled slot and a terminated key.
        unsafe { self.table.get::<IsInstalledFn>(apps::IS_INSTALLED)(key.as_ptr()) }
    }

    pub(super) fn identify(&self, pid: u32, key: &str) -> i32 {
        let key = cstr(key);
        // SAFETY: the SDK IdentifyApplication slot accepts this process id and live terminated key.
        unsafe { self.table.get::<IdentifyFn>(apps::IDENTIFY)(pid, key.as_ptr()) }
    }

    pub(super) fn set_auto_launch(&self, key: &str, on: bool) -> i32 {
        let key = cstr(key);
        // SAFETY: the SDK SetApplicationAutoLaunch slot accepts this live terminated key and boolean.
        unsafe { self.table.get::<SetAutoLaunchFn>(apps::SET_AUTO_LAUNCH)(key.as_ptr(), on) }
    }
}

impl Overlay<'_> {
    pub(super) fn create_dashboard(
        &self,
        key: &str,
        name: &str,
    ) -> Result<(OverlayHandle, OverlayHandle), i32> {
        let (key, name) = (cstr(key), cstr(name));
        let (mut main, mut thumb) = (0, 0);
        // SAFETY: the SDK dashboard slot receives terminated strings and two distinct writable handle outputs.
        let error = unsafe {
            self.table
                .get::<CreateDashboardFn>(overlay::CREATE_DASHBOARD)(
                key.as_ptr(),
                name.as_ptr(),
                &mut main,
                &mut thumb,
            )
        };
        if error == 0 {
            Ok((main, thumb))
        } else {
            Err(error)
        }
    }

    pub(super) fn set_from_file(&self, handle: OverlayHandle, path: &Path) -> i32 {
        let path = path_cstr(path);
        // SAFETY: the SDK image slot receives an opaque overlay handle and a live terminated path.
        unsafe { self.table.get::<SetFromFileFn>(overlay::SET_FROM_FILE)(handle, path.as_ptr()) }
    }

    pub(super) fn set_width(&self, handle: OverlayHandle, meters: f32) -> i32 {
        // SAFETY: this live table's SDK width slot accepts an opaque handle and a width in meters.
        unsafe { self.table.get::<SetWidthFn>(overlay::SET_WIDTH)(handle, meters) }
    }

    pub(super) fn set_rgba(
        &self,
        handle: OverlayHandle,
        pixels: &[u8],
        width: u32,
        height: u32,
    ) -> i32 {
        assert_eq!(
            pixels.len(),
            usize::try_from(width).expect("Windows pointer widths are at least 32 bits")
                * usize::try_from(height).expect("Windows pointer widths are at least 32 bits")
                * 4
        );
        // SAFETY: the SDK copies the RGBA buffer without modifying it; its asserted size covers all pixels.
        unsafe {
            self.table.get::<SetRawFn>(overlay::SET_RAW)(
                handle,
                pixels.as_ptr().cast_mut().cast(),
                width,
                height,
                4,
            )
        }
    }

    pub(super) fn is_visible(&self, handle: OverlayHandle) -> bool {
        // SAFETY: this live table's SDK visibility slot accepts an opaque overlay handle.
        unsafe { self.table.get::<IsVisibleFn>(overlay::IS_VISIBLE)(handle) }
    }

    pub(super) fn poll(&self, handle: OverlayHandle, event: &mut Event) -> bool {
        let size = u32::try_from(std::mem::size_of::<Event>())
            .expect("the OpenVR event has a fixed DWORD size");
        // SAFETY: the SDK polling slot receives a writable Event with its exact byte size.
        unsafe { self.table.get::<PollOverlayFn>(overlay::POLL_EVENT)(handle, event, size) }
    }

    pub(super) fn destroy(&self, handle: OverlayHandle) -> i32 {
        // SAFETY: the SDK destroy slot validates the opaque overlay handle in this live session.
        unsafe { self.table.get::<DestroyFn>(overlay::DESTROY)(handle) }
    }
}

impl System<'_> {
    pub(super) fn poll(&self, event: &mut Event) -> bool {
        let size = u32::try_from(std::mem::size_of::<Event>())
            .expect("the OpenVR event has a fixed DWORD size");
        // SAFETY: the SDK system polling slot receives a writable Event with its exact byte size.
        unsafe { self.table.get::<PollSystemFn>(system::POLL_EVENT)(event, size) }
    }

    pub(super) fn ack_quit(&self) {
        // SAFETY: this live system table uses the SDK AcknowledgeQuit_Exiting slot with no arguments.
        unsafe { self.table.get::<AckQuitFn>(system::ACK_QUIT)() }
    }
}

pub(super) fn runtime_dirs(vrpath: &str) -> Vec<PathBuf> {
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

pub(super) fn load_runtime() -> Result<OpenVr, String> {
    let base = std::env::var_os("LOCALAPPDATA").ok_or("no LOCALAPPDATA")?;
    let path = PathBuf::from(base)
        .join("openvr")
        .join("openvrpaths.vrpath");
    let vrpath = std::fs::read_to_string(&path).map_err(|error| {
        tracing::warn!(path = %path.display(), error = %error,
            "read SteamVR runtime paths {}: {error}", path.display());
        "SteamVR isn't installed".to_owned()
    })?;
    let mut last = String::from("SteamVR isn't installed");
    for dir in runtime_dirs(&vrpath) {
        match OpenVr::load(&dir) {
            Ok(vr) => return Ok(vr),
            Err(error) => last = error,
        }
    }
    Err(last)
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
        assert_eq!(runtime_dirs("{}").len(), 0);
        assert_eq!(runtime_dirs("garbage").len(), 0);
    }

    #[test]
    #[ignore = "needs SteamVR installed and closed"]
    fn background_init_leaves_steamvr_closed() {
        assert!(
            !crate::platform::process_running("vrserver.exe"),
            "close SteamVR first"
        );
        let mut vr = load_runtime().expect("SteamVR's openvr_api.dll loads");
        let error = vr.start(APP_BACKGROUND).expect_err("SteamVR isn't running");
        assert!(error.contains("NoServerForBackgroundApp"), "{error}");
        drop(vr);
        assert!(
            !crate::platform::process_running("vrserver.exe"),
            "the probe must not start SteamVR"
        );
    }

    #[test]
    fn event_matches_the_sdk_layout() {
        assert_eq!(std::mem::size_of::<Event>(), 64);
        assert_eq!(std::mem::offset_of!(Event, data), 16);
    }
}
