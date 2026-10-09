#![cfg(windows)]

use std::ffi::OsStr;
use std::io::Write;
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{HWND, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits, ReleaseDC, SRCCOPY, SelectObject,
};
use windows_sys::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowExW, GetWindowRect};

const BEAT: [u8; 3] = [0xfc, 0xa5, 0xa5];
const STILL: [u8; 3] = [0xf8, 0x71, 0x71];
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn wide(text: &str) -> Vec<u16> {
    OsStr::new(text).encode_wide().chain(Some(0)).collect()
}

fn taskbars() -> Vec<RECT> {
    let mut found = Vec::new();
    for class in ["Shell_TrayWnd", "Shell_SecondaryTrayWnd"] {
        let class = wide(class);
        let mut after: HWND = std::ptr::null_mut();
        loop {
            let bar = unsafe {
                FindWindowExW(
                    std::ptr::null_mut(),
                    after,
                    class.as_ptr(),
                    std::ptr::null(),
                )
            };
            if bar.is_null() {
                break;
            }
            let mut rect = RECT {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
            if unsafe { GetWindowRect(bar, &mut rect) } != 0
                && rect.right > rect.left
                && rect.bottom > rect.top
            {
                found.push(rect);
            }
            after = bar;
        }
    }
    found
}

fn capture(rect: &RECT) -> Vec<u8> {
    let (w, h) = (rect.right - rect.left, rect.bottom - rect.top);
    let mut pixels = vec![0u8; (w * h * 4) as usize];
    unsafe {
        let screen = GetDC(std::ptr::null_mut());
        let memory = CreateCompatibleDC(screen);
        let bitmap = CreateCompatibleBitmap(screen, w, h);
        let old = SelectObject(memory, bitmap);
        BitBlt(memory, 0, 0, w, h, screen, rect.left, rect.top, SRCCOPY);
        let mut info: BITMAPINFO = std::mem::zeroed();
        info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = w;
        info.bmiHeader.biHeight = -h;
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        info.bmiHeader.biCompression = BI_RGB;
        GetDIBits(
            memory,
            bitmap,
            0,
            h as u32,
            pixels.as_mut_ptr().cast(),
            &mut info,
            DIB_RGB_COLORS,
        );
        SelectObject(memory, old);
        DeleteObject(bitmap);
        DeleteDC(memory);
        ReleaseDC(std::ptr::null_mut(), screen);
    }
    pixels
}

fn count(pixels: &[u8], colour: [u8; 3]) -> usize {
    pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|&&[b, g, r, _]| {
            r.abs_diff(colour[0]) <= 12
                && g.abs_diff(colour[1]) <= 12
                && b.abs_diff(colour[2]) <= 12
        })
        .count()
}

struct Scratch {
    root: PathBuf,
    shortcut: Option<PathBuf>,
    child: Option<Child>,
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(shortcut) = &self.shortcut {
            let _ = std::fs::remove_file(shortcut);
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn start_menu_shortcut(name: &str, target: &Path) -> PathBuf {
    let dir = PathBuf::from(std::env::var_os("APPDATA").unwrap())
        .join(r"Microsoft\Windows\Start Menu\Programs");
    let link = dir.join(format!("{name}.lnk"));
    let script = format!(
        "$s=(New-Object -ComObject WScript.Shell).CreateShortcut('{}'); $s.TargetPath='{}'; $s.Save()",
        link.display(),
        target.display()
    );
    let status = Command::new("powershell.exe")
        .args(["-NoProfile", "-Command", &script])
        .status()
        .unwrap();
    assert!(status.success(), "creating {}", link.display());
    link
}

fn post(port: u16, bpm: u16) -> bool {
    let Ok(mut stream) = TcpStream::connect(("127.0.0.1", port)) else {
        return false;
    };
    let body = bpm.to_string();
    let request = format!(
        "POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).is_ok()
}

#[test]
#[ignore = "needs an interactive Windows desktop with a visible taskbar"]
fn the_taskbar_icon_beats_when_a_start_menu_shortcut_points_at_the_exe() {
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    let source = std::env::var_os("HEARTWIRE_EXE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_heartwire")));
    let root = std::env::temp_dir().join(format!("heartwire-taskbar-{}", std::process::id()));
    let mut scratch = Scratch {
        root: root.clone(),
        shortcut: None,
        child: None,
    };
    let _ = std::fs::remove_dir_all(&root);
    let data = root.join("appdata").join("heartwire");
    std::fs::create_dir_all(&data).unwrap();
    let in_place = std::env::var_os("HEARTWIRE_IN_PLACE").is_some();
    let exe = if in_place {
        source.clone()
    } else {
        let install = root.join("Heartwire");
        std::fs::create_dir_all(&install).unwrap();
        let exe = install.join("heartwire.exe");
        std::fs::copy(&source, &exe).unwrap();
        exe
    };
    let install = exe.parent().unwrap().to_path_buf();
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let osc = UdpSocket::bind("127.0.0.1:0").unwrap();
    osc.set_nonblocking(true).unwrap();
    let osc_port = osc.local_addr().unwrap().port();
    std::fs::write(
        data.join("config.json"),
        format!(
            r#"{{"service_type":"http","http_server_port":{port},"osc_client_port":{osc_port},"check_updates":false,"steamvr_autostart":false}}"#
        ),
    )
    .unwrap();
    if !in_place && std::env::var_os("HEARTWIRE_NO_SHORTCUT").is_none() {
        scratch.shortcut = Some(start_menu_shortcut(
            &format!("Heartwire taskbar test {}", std::process::id()),
            &exe,
        ));
        let wait = std::env::var("HEARTWIRE_SHORTCUT_WAIT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(30);
        std::thread::sleep(Duration::from_secs(wait));
    }
    scratch.child = Some(
        Command::new(&exe)
            .arg("--minimized")
            .env("APPDATA", root.join("appdata"))
            .current_dir(&install)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(15);
    while !post(port, 120) {
        assert!(Instant::now() < deadline, "the HTTP source never listened");
        std::thread::sleep(Duration::from_millis(100));
    }
    let feeding = Arc::new(AtomicBool::new(true));
    let feeder = {
        let feeding = feeding.clone();
        std::thread::spawn(move || {
            while feeding.load(Ordering::Relaxed) {
                post(port, 120);
                std::thread::sleep(Duration::from_millis(500));
            }
        })
    };
    std::thread::sleep(Duration::from_millis(1500));
    let bars = taskbars();
    assert!(!bars.is_empty(), "no taskbar found");
    let mut beat = Vec::new();
    let mut still = Vec::new();
    let end = Instant::now() + Duration::from_secs(6);
    while Instant::now() < end {
        let (mut b, mut s) = (0, 0);
        for bar in &bars {
            let pixels = capture(bar);
            b += count(&pixels, BEAT);
            s += count(&pixels, STILL);
        }
        beat.push(b);
        still.push(s);
        std::thread::sleep(Duration::from_millis(25));
    }
    feeding.store(false, Ordering::Relaxed);
    feeder.join().unwrap();
    let mut packet = [0u8; 128];
    let mut packets = 0;
    while osc.recv(&mut packet).is_ok() {
        packets += 1;
    }
    assert!(
        packets > 0,
        "the app never sent OSC, so it was not reading the feed"
    );
    let most = beat.iter().copied().max().unwrap_or(0);
    let least = beat.iter().copied().min().unwrap_or(0);
    let lit = beat.iter().filter(|&&b| b >= most / 2).count();
    let dark = beat.iter().filter(|&&b| b * 4 <= most).count();
    let flashes = beat
        .windows(2)
        .filter(|w| w[0] < most / 2 && w[1] >= most / 2)
        .count();
    eprintln!(
        "{packets} OSC packets, {} frames, beat-colour pixels {least}..{most}, rising edges {flashes}, lit frames {lit}, dark frames {dark}, still-colour pixels {}..{}",
        beat.len(),
        still.iter().min().unwrap_or(&0),
        still.iter().max().unwrap_or(&0)
    );
    assert!(
        most >= 20 && lit >= 2 && dark >= 2,
        "the taskbar icon did not beat: beat-colour pixels per frame {beat:?}"
    );
}
