use std::path::{Path, PathBuf};

use serde_json::Value;

use super::openvr::{self, OpenVr};
use super::{APP_KEY, LAUNCH_FLAG};
use crate::config::{Config, Store};
use crate::log;

const OK: i32 = 0;
const UNREACHABLE: i32 = 2;
const NOT_INSTALLED: i32 = 3;
const FAILED: i32 = 4;

const MANIFEST: &str = "heartwire.vrmanifest";

const ICON: &str = "heartwire-icon.png";

const ICON_BYTES: &[u8] = include_bytes!("../../assets/icon.png");

fn json_string(text: &str) -> String {
    serde_json::Value::String(text.to_owned()).to_string()
}

pub fn manifest(binary: &str, image: &str) -> String {
    format!(
        "{{\n  \"source\": \"builtin\",\n  \"applications\": [\n    {{\n      \"app_key\": {},\n      \"launch_type\": \"binary\",\n      \"binary_path_windows\": {},\n      \"arguments\": {},\n      \"is_dashboard_overlay\": true,\n      \"image_path\": {},\n      \"strings\": {{\n        \"en_us\": {{\n          \"name\": \"Heartwire\",\n          \"description\": \"Heart rate to VRChat over OSC\"\n        }}\n      }}\n    }}\n  ]\n}}\n",
        json_string(APP_KEY),
        json_string(binary),
        json_string(LAUNCH_FLAG),
        json_string(image),
    )
}

pub(super) fn write_files(data_dir: &Path) -> Result<(PathBuf, PathBuf), String> {
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

fn same_path(a: &Path, b: &Path) -> bool {
    fn normal(path: &Path) -> String {
        let full = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let text = full.display().to_string();
        text.strip_prefix(r"\\?\").unwrap_or(&text).to_lowercase()
    }
    normal(a) == normal(b)
}

fn strings<'a>(value: &'a Value, key: &str) -> impl Iterator<Item = &'a str> {
    value[key]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|s| !s.is_empty())
}

fn registered_in(vrpath: &str, read: &dyn Fn(&Path) -> Option<String>) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();
    let Ok(paths) = serde_json::from_str::<Value>(vrpath) else {
        return found;
    };
    for config in strings(&paths, "config") {
        let Some(text) = read(&Path::new(config).join("appconfig.json")) else {
            continue;
        };
        let Ok(apps) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        for path in strings(&apps, "manifest_paths").map(PathBuf::from) {
            if !found.iter().any(|p| same_path(p, &path)) {
                found.push(path);
            }
        }
    }
    found
}

fn registered() -> Option<Vec<PathBuf>> {
    let base = std::env::var_os("LOCALAPPDATA")?;
    let vrpath = PathBuf::from(base)
        .join("openvr")
        .join("openvrpaths.vrpath");
    let text = std::fs::read_to_string(vrpath).ok()?;
    Some(registered_in(&text, &|p| std::fs::read_to_string(p).ok()))
}

fn app_key(manifest_text: &str) -> Option<String> {
    let value: Value = serde_json::from_str(manifest_text).ok()?;
    value["applications"]
        .as_array()?
        .iter()
        .find_map(|app| app["app_key"].as_str())
        .map(str::to_owned)
}

fn key_of(manifest: &Path) -> Option<String> {
    std::fs::read_to_string(manifest)
        .ok()
        .and_then(|text| app_key(&text))
}

struct Plan {
    ours: Option<PathBuf>,
    rivals: Vec<PathBuf>,
}

fn plan(manifest: &Path, registered: &[PathBuf], key_of: &dyn Fn(&Path) -> Option<String>) -> Plan {
    let mut plan = Plan {
        ours: None,
        rivals: Vec::new(),
    };
    for path in registered {
        if same_path(path, manifest) {
            plan.ours = Some(path.clone());
        } else if key_of(path).as_deref() == Some(APP_KEY) {
            plan.rivals.push(path.clone());
        }
    }
    plan
}

fn remember(data_dir: &Path, registered: bool) {
    if !data_dir.join("config.json").is_file() {
        return;
    }
    let store = Store::new(data_dir);
    let mut config = store.load().config;
    if config.steamvr_registered != registered {
        config.steamvr_registered = registered;
        store.save(&config);
    }
}

fn connect() -> Result<OpenVr, i32> {
    let mut vr = openvr::load_runtime().map_err(|error| {
        log::write(&format!("steamvr: {error}"));
        NOT_INSTALLED
    })?;
    vr.start(openvr::APP_UTILITY).map_err(|error| {
        log::write(&format!("steamvr: can't reach SteamVR: {error}"));
        UNREACHABLE
    })?;
    Ok(vr)
}

pub fn register_for_setup(data_dir: &Path) -> i32 {
    let enabled = std::fs::read_to_string(data_dir.join("config.json"))
        .map(|text| Config::from_json(&text).steamvr_autostart)
        .unwrap_or(false);
    if !enabled {
        return OK;
    }
    let Some(registered) = registered() else {
        log::write("steamvr: SteamVR isn't installed");
        return NOT_INSTALLED;
    };
    let manifest = match write_files(data_dir) {
        Ok((manifest, _)) => manifest,
        Err(error) => {
            log::write(&format!("steamvr: writing the manifest failed: {error}"));
            return FAILED;
        }
    };
    let plan = plan(&manifest, &registered, &key_of);
    if plan.ours.is_some() && plan.rivals.is_empty() {
        remember(data_dir, true);
        return OK;
    }
    let vr = match connect() {
        Ok(vr) => vr,
        Err(code) => return code,
    };
    match apply_registration(&vr, true, data_dir) {
        Ok(()) => {
            remember(data_dir, true);
            log::write("steamvr: added to SteamVR start-up by the installer");
            OK
        }
        Err(error) => {
            log::write(&format!("steamvr: {error}"));
            FAILED
        }
    }
}

pub fn unregister_for_setup(data_dir: &Path) -> i32 {
    let Some(registered) = registered() else {
        return OK;
    };
    let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf))
    else {
        return FAILED;
    };
    let Some(stored) = plan(&dir.join(MANIFEST), &registered, &|_| None).ours else {
        return OK;
    };
    let vr = match connect() {
        Ok(vr) => vr,
        Err(code) => return code,
    };
    let error = match vr.applications() {
        Ok(apps) => apps.remove_manifest(&stored),
        Err(error) => {
            log::write(&format!("steamvr: {error}"));
            return FAILED;
        }
    };
    if error != 0 {
        log::write(&format!(
            "steamvr: removing {} failed ({error})",
            stored.display()
        ));
        return FAILED;
    }
    remember(data_dir, false);
    log::write(&format!(
        "steamvr: removed {} for the uninstaller",
        stored.display()
    ));
    OK
}

pub(super) fn apply_registration(vr: &OpenVr, enable: bool, data_dir: &Path) -> Result<(), String> {
    let apps = vr.applications()?;
    if enable {
        let (manifest, _) = write_files(data_dir)?;
        let registered = registered().unwrap_or_default();
        for rival in plan(&manifest, &registered, &key_of).rivals {
            let error = apps.remove_manifest(&rival);
            log::write(&format!(
                "steamvr: removed another copy's registration {} ({error})",
                rival.display()
            ));
        }
        let error = apps.add_manifest(&manifest);
        if error != 0 {
            return Err(format!("adding the manifest failed ({error})"));
        }
        let error = apps.set_auto_launch(APP_KEY, true);
        if error != 0 {
            return Err(format!("turning on auto launch failed ({error})"));
        }
        return Ok(());
    }
    if apps.is_installed(APP_KEY) {
        apps.set_auto_launch(APP_KEY, false);
    }
    let beside = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf));
    for dir in [beside, Some(data_dir.to_path_buf())].into_iter().flatten() {
        let path = dir.join(MANIFEST);
        if path.is_file() {
            apps.remove_manifest(&path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::update::testing::scratch;

    #[test]
    fn reads_every_registered_manifest_once() {
        let vrpath = r#"{"config":["C:\\Steam\\config","D:\\Other"],"runtime":[]}"#;
        let read = |path: &Path| match path.to_str()? {
            r"C:\Steam\config\appconfig.json" => Some(
                r#"{"manifest_paths":["C:\\A\\heartwire.vrmanifest","C:\\B\\app.vrmanifest",""]}"#
                    .to_owned(),
            ),
            r"D:\Other\appconfig.json" => Some(
                r#"{"manifest_paths":["c:\\a\\HEARTWIRE.vrmanifest","D:\\C\\x.vrmanifest"]}"#
                    .to_owned(),
            ),
            _ => None,
        };
        assert_eq!(
            registered_in(vrpath, &read),
            vec![
                PathBuf::from(r"C:\A\heartwire.vrmanifest"),
                PathBuf::from(r"C:\B\app.vrmanifest"),
                PathBuf::from(r"D:\C\x.vrmanifest"),
            ]
        );
        assert!(registered_in("garbage", &read).is_empty());
        assert!(registered_in(r#"{"config":["C:\\Missing"]}"#, &read).is_empty());
    }

    #[test]
    fn finds_this_copy_and_other_copies_by_app_key() {
        assert_eq!(
            app_key(&manifest("heartwire.exe", "icon.png")).as_deref(),
            Some(APP_KEY)
        );
        assert_eq!(app_key("{}"), None);
        let registered = [
            PathBuf::from(r"C:\Old\heartwire.vrmanifest"),
            PathBuf::from(r"C:\Other\app.vrmanifest"),
            PathBuf::from(r"C:\USERS\ME\Heartwire\HEARTWIRE.VRMANIFEST"),
        ];
        let key_of = |path: &Path| {
            path.starts_with(r"C:\Old")
                .then(|| APP_KEY.to_owned())
                .or_else(|| Some("someone.else".to_owned()))
        };
        let plan = plan(
            Path::new(r"C:\Users\me\Heartwire\heartwire.vrmanifest"),
            &registered,
            &key_of,
        );
        assert_eq!(plan.ours, Some(registered[2].clone()));
        assert_eq!(plan.rivals, vec![registered[0].clone()]);
        let none = super::plan(Path::new(r"C:\New\heartwire.vrmanifest"), &[], &key_of);
        assert!(none.ours.is_none() && none.rivals.is_empty());
    }

    #[test]
    fn remembering_the_registration_keeps_every_other_setting() {
        let dir = scratch("remember");
        remember(&dir, false);
        assert!(
            !dir.join("config.json").exists(),
            "no settings file is created"
        );
        let store = Store::new(&dir);
        let mut config =
            Config::from_json(r#"{"osc_client_port": 9123, "steamvr_autostart": true}"#);
        assert_eq!(config.osc_client_port, 9123);
        config.steamvr_registered = true;
        store.save(&config);
        remember(&dir, false);
        let after = store.load().config;
        assert!(!after.steamvr_registered);
        config.steamvr_registered = false;
        assert_eq!(
            serde_json::to_value(&after).unwrap(),
            serde_json::to_value(&config).unwrap()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn manifest_is_valid_json_with_the_overlay_flag() {
        let text = manifest(r#"C:\it's "here"\heartwire.exe"#, "heartwire-icon.png");
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        let app = &value["applications"][0];
        assert_eq!(app["app_key"], APP_KEY);
        assert_eq!(app["is_dashboard_overlay"], true);
        assert_eq!(app["launch_type"], "binary");
        assert_eq!(app["arguments"], LAUNCH_FLAG);
        assert_eq!(
            app["binary_path_windows"],
            r#"C:\it's "here"\heartwire.exe"#
        );
        assert_eq!(app["image_path"], "heartwire-icon.png");
        assert_eq!(value["source"], "builtin");
    }
}
