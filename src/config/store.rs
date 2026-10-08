use std::path::{Path, PathBuf};

use super::Config;

pub fn data_dir() -> PathBuf {
    #[cfg(windows)]
    let base = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(target_os = "macos")]
    let base =
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"));
    #[cfg(all(unix, not(target_os = "macos")))]
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));
    base.unwrap_or_else(std::env::temp_dir).join("heartwire")
}

fn upstream_config(base: &Path) -> Option<PathBuf> {
    let path = base
        .parent()?
        .join("me.kamyu.hr-osc")
        .join("data")
        .join("config.json");
    path.is_file().then_some(path)
}

pub struct Store {
    path: PathBuf,
}

impl Store {
    pub fn new(dir: &Path) -> Store {
        Store {
            path: dir.join("config.json"),
        }
    }

    pub fn load(&self) -> Config {
        if let Ok(text) = std::fs::read_to_string(&self.path) {
            return Config::from_json(&text);
        }
        let dir = self.path.parent().unwrap_or(Path::new("."));
        let config = upstream_config(dir)
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map(|text| {
                crate::log::write("imported settings from hr-osc");
                Config::from_json(&text)
            })
            .unwrap_or_default();
        self.save(&config);
        config
    }

    pub fn save(&self, config: &Config) {
        let Ok(text) = serde_json::to_string_pretty(config) else {
            return;
        };
        let tmp = self.path.with_extension("json.tmp");
        let result = std::fs::create_dir_all(self.path.parent().unwrap_or(Path::new(".")))
            .and_then(|_| std::fs::write(&tmp, text + "\n"))
            .and_then(|_| std::fs::rename(&tmp, &self.path));
        if let Err(error) = result {
            crate::log::write(&format!("saving settings: {error}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Service;

    #[test]
    fn store_round_trips_and_imports_upstream() {
        let root = std::env::temp_dir().join(format!("heartwire-test-{}", std::process::id()));
        let ours = root.join("heartwire");
        let theirs = root.join("me.kamyu.hr-osc").join("data");
        std::fs::create_dir_all(&theirs).unwrap();
        std::fs::write(
            theirs.join("config.json"),
            r#"{"service_type":"http","max_heart_rate":190}"#,
        )
        .unwrap();
        let store = Store::new(&ours);
        let loaded = store.load();
        assert_eq!(loaded.service_type, Service::Http);
        assert_eq!(loaded.max_heart_rate, 190);
        let mut changed = loaded.clone();
        changed.max_heart_rate = 210;
        store.save(&changed);
        assert_eq!(Store::new(&ours).load(), changed);
        let _ = std::fs::remove_file(ours.join("config.json"));
        let _ = std::fs::remove_file(theirs.join("config.json"));
        let _ = std::fs::remove_dir(&theirs);
        let _ = std::fs::remove_dir(root.join("me.kamyu.hr-osc"));
        let _ = std::fs::remove_dir(&ours);
        let _ = std::fs::remove_dir(&root);
    }
}
