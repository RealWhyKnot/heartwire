use std::path::{Path, PathBuf};

use super::openvr::{self, OpenVr};
use super::{APP_KEY, LAUNCH_FLAG};

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

pub(super) fn apply_registration(vr: &OpenVr, enable: bool, data_dir: &Path) -> Result<(), String> {
    let apps = vr.table("IVRApplications_007")?;
    let key = openvr::cstr(APP_KEY);
    unsafe {
        let set_auto: openvr::SetAutoLaunchFn = apps.get(openvr::apps::SET_AUTO_LAUNCH);
        if enable {
            let (manifest, _) = write_files(data_dir)?;
            let path = openvr::cstr(&manifest.display().to_string());
            let add: openvr::AddManifestFn = apps.get(openvr::apps::ADD_MANIFEST);
            let error = add(path.as_ptr(), false);
            if error != 0 {
                return Err(format!("adding the manifest failed ({error})"));
            }
            let error = set_auto(key.as_ptr(), true);
            if error != 0 {
                return Err(format!("turning on auto launch failed ({error})"));
            }
        } else {
            let installed: openvr::IsInstalledFn = apps.get(openvr::apps::IS_INSTALLED);
            if installed(key.as_ptr()) {
                set_auto(key.as_ptr(), false);
            }
            let remove: openvr::RemoveManifestFn = apps.get(openvr::apps::REMOVE_MANIFEST);
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
                    remove(openvr::cstr(&path.display().to_string()).as_ptr());
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
