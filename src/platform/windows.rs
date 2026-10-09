use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::ptr::null;

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_FILE_NOT_FOUND, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, REG_SZ, RRF_RT_REG_SZ, RegCloseKey,
    RegDeleteKeyValueW, RegEnumValueW, RegGetValueW, RegOpenKeyExW, RegSetKeyValueW,
};
use windows_sys::Win32::UI::Shell::{SetCurrentProcessExplicitAppUserModelID, ShellExecuteW};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    FindWindowW, SW_RESTORE, SW_SHOWNORMAL, SetForegroundWindow, ShowWindow,
};

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE: &str = "Heartwire";
const APP_ID: &str = "WhyKnot.Heartwire";

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

fn same_name(exe: &[u16], name: &str) -> bool {
    let end = exe.iter().position(|&c| c == 0).unwrap_or(exe.len());
    let lower = |c: u16| match u8::try_from(c) {
        Ok(b) => u16::from(b.to_ascii_lowercase()),
        Err(_) => c,
    };
    exe[..end]
        .iter()
        .copied()
        .map(lower)
        .eq(name.encode_utf16().map(lower))
}

pub fn process_running(name: &str) -> bool {
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return false;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut found = false;
        let mut more = Process32FirstW(snapshot, &mut entry) != 0;
        while more && !found {
            found = same_name(&entry.szExeFile, name);
            more = Process32NextW(snapshot, &mut entry) != 0;
        }
        CloseHandle(snapshot);
        found
    }
}

pub fn claim_app_id() {
    let id = wide(APP_ID);
    unsafe { SetCurrentProcessExplicitAppUserModelID(id.as_ptr()) };
}

pub fn serial_ports_key() -> Option<Vec<u16>> {
    let path = wide(r"HARDWARE\DEVICEMAP\SERIALCOMM");
    let mut key: HKEY = std::ptr::null_mut();
    let status = unsafe { RegOpenKeyExW(HKEY_LOCAL_MACHINE, path.as_ptr(), 0, KEY_READ, &mut key) };
    if status == ERROR_FILE_NOT_FOUND {
        return Some(Vec::new());
    }
    if status != ERROR_SUCCESS {
        return None;
    }
    let mut out = Vec::new();
    let mut name = [0u16; 256];
    let mut data = [0u16; 128];
    let mut index = 0;
    let complete = loop {
        let mut name_len = name.len() as u32;
        let mut data_len = (data.len() * 2) as u32;
        let status = unsafe {
            RegEnumValueW(
                key,
                index,
                name.as_mut_ptr(),
                &mut name_len,
                std::ptr::null(),
                std::ptr::null_mut(),
                data.as_mut_ptr().cast(),
                &mut data_len,
            )
        };
        if status == ERROR_NO_MORE_ITEMS {
            break true;
        }
        if status != ERROR_SUCCESS {
            break false;
        }
        out.extend_from_slice(&name[..name_len as usize]);
        out.push(0);
        out.extend_from_slice(&data[..data_len as usize / 2]);
        out.push(0);
        index += 1;
    };
    unsafe { RegCloseKey(key) };
    complete.then_some(out)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_names_compare_without_case() {
        let mut exe = [0u16; 260];
        for (slot, c) in exe.iter_mut().zip("VRServer.exe".encode_utf16()) {
            *slot = c;
        }
        assert!(same_name(&exe, "vrserver.exe"));
        assert!(!same_name(&exe, "vrserver.ex"));
        assert!(!same_name(&exe, "vrserver.exe2"));
        assert!(!same_name(&[0u16; 4], "a"));
    }

    #[test]
    fn this_test_process_is_found() {
        let exe = std::env::current_exe().unwrap();
        let name = exe.file_name().unwrap().to_str().unwrap();
        assert!(process_running(name));
        assert!(!process_running("no-such-process-heartwire.exe"));
    }

    #[test]
    fn the_app_id_is_claimed() {
        use windows_sys::Win32::System::Com::CoTaskMemFree;
        use windows_sys::Win32::UI::Shell::GetCurrentProcessExplicitAppUserModelID;
        claim_app_id();
        let mut id = std::ptr::null_mut();
        let result = unsafe { GetCurrentProcessExplicitAppUserModelID(&mut id) };
        assert_eq!(result, 0);
        let text = unsafe {
            let len = (0..).take_while(|&i| *id.add(i) != 0).count();
            let text = String::from_utf16_lossy(std::slice::from_raw_parts(id, len));
            CoTaskMemFree(id.cast());
            text
        };
        assert_eq!(text, APP_ID);
    }
}
