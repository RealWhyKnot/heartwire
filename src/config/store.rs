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

fn hr_osc_config(base: &Path) -> Option<PathBuf> {
    let path = base
        .parent()?
        .join("me.kamyu.hr-osc")
        .join("data")
        .join("config.json");
    path.is_file().then_some(path)
}

pub struct Loaded {
    pub config: Config,
    pub hr_osc: Option<PathBuf>,
}

pub fn read_hr_osc(path: &Path) -> Option<Config> {
    std::fs::read_to_string(path)
        .ok()
        .map(|text| Config::from_json(&text))
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

    pub fn load(&self) -> Loaded {
        if let Ok(text) = std::fs::read_to_string(&self.path) {
            return Loaded {
                config: Config::from_json(&text),
                hr_osc: None,
            };
        }
        let config = Config::default();
        self.save(&config);
        Loaded {
            config,
            hr_osc: hr_osc_config(self.path.parent().unwrap_or(Path::new("."))),
        }
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

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Scratch {
            let root =
                std::env::temp_dir().join(format!("heartwire-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).unwrap();
            Scratch(root)
        }

        fn hr_osc(&self, json: &str) -> PathBuf {
            let dir = self.0.join("me.kamyu.hr-osc").join("data");
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join("config.json");
            std::fs::write(&path, json).unwrap();
            path
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn first_run_offers_hr_osc_settings_without_applying_them() {
        let scratch = Scratch::new("first-run");
        let found = scratch.hr_osc(r#"{"service_type":"http","max_heart_rate":190}"#);
        let loaded = Store::new(&scratch.0.join("heartwire")).load();
        assert_eq!(loaded.config, Config::default());
        assert_eq!(loaded.hr_osc.as_deref(), Some(found.as_path()));
        let imported = read_hr_osc(&found).unwrap();
        assert_eq!(imported.service_type, Service::Http);
        assert_eq!(imported.max_heart_rate, 190);
    }

    #[test]
    fn the_offer_is_made_only_once() {
        let scratch = Scratch::new("second-run");
        scratch.hr_osc(r#"{"service_type":"http"}"#);
        let store = Store::new(&scratch.0.join("heartwire"));
        assert!(store.load().hr_osc.is_some());
        assert!(store.load().hr_osc.is_none());
    }

    #[test]
    fn no_offer_without_hr_osc() {
        let scratch = Scratch::new("no-hr-osc");
        assert!(
            Store::new(&scratch.0.join("heartwire"))
                .load()
                .hr_osc
                .is_none()
        );
    }

    #[test]
    fn saved_settings_round_trip() {
        let scratch = Scratch::new("round-trip");
        let store = Store::new(&scratch.0.join("heartwire"));
        let mut config = store.load().config;
        config.max_heart_rate = 210;
        store.save(&config);
        assert_eq!(
            Store::new(&scratch.0.join("heartwire")).load().config,
            config
        );
    }
}
