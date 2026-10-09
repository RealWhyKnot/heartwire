mod store;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub use store::{Store, data_dir, read_hr_osc};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Service {
    Auto,
    Bluetooth,
    Pico,
    Http,
    #[serde(rename = "stromno", alias = "pulsoid")]
    Pulsoid,
}

impl Service {
    pub const ALL: [Service; 5] = [
        Service::Auto,
        Service::Bluetooth,
        Service::Pico,
        Service::Http,
        Service::Pulsoid,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Service::Auto => "auto",
            Service::Bluetooth => "bluetooth",
            Service::Pico => "pico",
            Service::Http => "http",
            Service::Pulsoid => "stromno",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Service::Auto => "Auto (Bluetooth / Pico)",
            Service::Bluetooth => "Bluetooth",
            Service::Pico => "Pico (USB)",
            Service::Http => "HTTP",
            Service::Pulsoid => "Pulsoid / Stromno",
        }
    }

    pub fn index(self) -> usize {
        Service::ALL.iter().position(|s| *s == self).unwrap_or(0)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Config {
    pub service_type: Service,
    pub http_server_port: u16,
    pub stromno_widget_id: String,
    pub osc_path_connected: String,
    pub osc_path_percent: String,
    pub connected_timeout: u32,
    pub max_heart_rate: u32,
    pub osc_client_host: String,
    pub osc_client_port: u16,
    pub bluetooth_device: String,
    pub pico_strap_name: String,
    pub check_updates: bool,
    pub skipped_update: String,
    pub steamvr_autostart: bool,
    pub steamvr_registered: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            service_type: Service::Auto,
            http_server_port: 8080,
            stromno_widget_id: String::new(),
            osc_path_connected: "/avatar/parameters/hr_connected".into(),
            osc_path_percent: "/avatar/parameters/hr_percent".into(),
            connected_timeout: 10,
            max_heart_rate: 200,
            osc_client_host: "127.0.0.1".into(),
            osc_client_port: 9000,
            bluetooth_device: String::new(),
            pico_strap_name: String::new(),
            check_updates: true,
            skipped_update: String::new(),
            steamvr_autostart: false,
            steamvr_registered: false,
        }
    }
}

impl Config {
    pub fn from_json(text: &str) -> Config {
        let mut merged = match serde_json::to_value(Config::default()) {
            Ok(Value::Object(map)) => map,
            _ => Map::new(),
        };
        let defaults = merged.clone();
        if let Ok(Value::Object(found)) = serde_json::from_str::<Value>(text) {
            for (key, value) in found {
                let Some(value) = defaults.get(&key).and_then(|d| coerce(d, value)) else {
                    continue;
                };
                let mut probe = defaults.clone();
                probe.insert(key.clone(), value.clone());
                if serde_json::from_value::<Config>(Value::Object(probe)).is_ok() {
                    merged.insert(key, value);
                }
            }
        }
        let mut config: Config = serde_json::from_value(Value::Object(merged)).unwrap_or_default();
        config.clamp();
        config
    }

    fn clamp(&mut self) {
        let d = Config::default();
        if self.http_server_port == 0 {
            self.http_server_port = d.http_server_port;
        }
        if self.osc_client_port == 0 {
            self.osc_client_port = d.osc_client_port;
        }
        if self.max_heart_rate == 0 {
            self.max_heart_rate = d.max_heart_rate;
        }
        if self.connected_timeout == 0 {
            self.connected_timeout = d.connected_timeout;
        }
    }

    pub fn set(&mut self, key: &str, value: &str) -> bool {
        fn update<T: PartialEq>(field: &mut T, value: Option<T>) -> bool {
            match value {
                Some(value) if *field != value => {
                    *field = value;
                    true
                }
                _ => false,
            }
        }
        fn text(field: &mut String, value: &str) -> bool {
            if field == value {
                return false;
            }
            value.clone_into(field);
            true
        }
        let value = value.trim();
        let number = value.parse::<u32>().ok().filter(|n| *n > 0);
        let port = value.parse::<u16>().ok().filter(|n| *n > 0);
        match key {
            "connected_timeout" => update(&mut self.connected_timeout, number),
            "max_heart_rate" => update(&mut self.max_heart_rate, number),
            "http_server_port" => update(&mut self.http_server_port, port),
            "osc_client_port" => update(&mut self.osc_client_port, port),
            "osc_client_host" => text(&mut self.osc_client_host, value),
            "osc_path_connected" => text(&mut self.osc_path_connected, value),
            "osc_path_percent" => text(&mut self.osc_path_percent, value),
            "stromno_widget_id" => text(&mut self.stromno_widget_id, value),
            "pico_strap_name" => text(&mut self.pico_strap_name, value),
            _ => false,
        }
    }

    pub fn source_key(&self) -> (Service, u16, &str, &str) {
        (
            self.service_type,
            self.http_server_port,
            self.stromno_widget_id.trim(),
            self.bluetooth_device.trim(),
        )
    }
}

fn coerce(default: &Value, value: Value) -> Option<Value> {
    match (default, value) {
        (Value::Number(_), Value::Number(n)) => n
            .as_f64()
            .filter(|f| f.is_finite() && *f >= 0.0)
            .map(|f| Value::from(f.round() as u64)),
        (Value::Number(_), Value::String(s)) => s
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|f| f.is_finite() && *f >= 0.0)
            .map(|f| Value::from(f.round() as u64)),
        (Value::String(_), Value::String(s)) => Some(Value::String(s)),
        (Value::Bool(_), Value::Bool(b)) => Some(Value::Bool(b)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upstream_config_keeps_its_values() {
        let text = r#"{
          "service_type": "http",
          "http_server_port": 8080,
          "stromno_widget_id": "3f2b8c1e-5d4a-4e6f-9a7b-2c1d0e9f8a7b",
          "osc_path_connected": "/avatar/parameters/hr_connected",
          "osc_path_percent": "/avatar/parameters/hr_percent",
          "connected_timeout": 60,
          "max_heart_rate": 230,
          "osc_client_host": "127.0.0.1",
          "osc_client_port": 9000
        }"#;
        let c = Config::from_json(text);
        assert_eq!(c.service_type, Service::Http);
        assert_eq!(c.connected_timeout, 60);
        assert_eq!(c.max_heart_rate, 230);
        assert_eq!(c.stromno_widget_id, "3f2b8c1e-5d4a-4e6f-9a7b-2c1d0e9f8a7b");
        assert!(c.check_updates);
        assert_eq!(c.bluetooth_device, "");
    }

    #[test]
    fn one_bad_field_does_not_lose_the_rest() {
        let c = Config::from_json(
            r#"{"service_type":"carrier pigeon","max_heart_rate":"180","osc_client_port":-4,"connected_timeout":true,"osc_client_host":"10.0.0.2"}"#,
        );
        assert_eq!(c.service_type, Service::Auto);
        assert_eq!(c.max_heart_rate, 180);
        assert_eq!(c.osc_client_port, 9000);
        assert_eq!(c.connected_timeout, 10);
        assert_eq!(c.osc_client_host, "10.0.0.2");
    }

    #[test]
    fn garbage_gives_defaults() {
        assert_eq!(Config::from_json("not json"), Config::default());
        assert_eq!(Config::from_json("[]"), Config::default());
        assert_eq!(
            Config::from_json(r#"{"max_heart_rate":0}"#).max_heart_rate,
            200
        );
    }

    #[test]
    fn numbers_must_be_positive() {
        let mut c = Config::default();
        assert!(c.set("max_heart_rate", " 185 "));
        assert_eq!(c.max_heart_rate, 185);
        assert!(!c.set("max_heart_rate", "0"));
        assert!(!c.set("max_heart_rate", "fast"));
        assert_eq!(c.max_heart_rate, 185);
    }

    #[test]
    fn ports_must_fit_in_sixteen_bits() {
        let mut c = Config::default();
        assert!(c.set("osc_client_port", "9001"));
        assert_eq!(c.osc_client_port, 9001);
        assert!(!c.set("osc_client_port", "70000"));
        assert!(!c.set("http_server_port", "0"));
        assert_eq!(c.http_server_port, 8080);
    }

    #[test]
    fn text_fields_are_trimmed() {
        let mut c = Config::default();
        assert!(c.set("osc_client_host", "  192.168.1.5 "));
        assert_eq!(c.osc_client_host, "192.168.1.5");
        assert!(c.set("pico_strap_name", " Polar "));
        assert_eq!(c.pico_strap_name, "Polar");
    }

    #[test]
    fn setting_the_same_value_is_not_a_change() {
        let mut c = Config::default();
        for (key, value) in [
            ("connected_timeout", "10"),
            ("max_heart_rate", " 200"),
            ("http_server_port", "8080"),
            ("osc_client_port", "9000 "),
            ("osc_client_host", "127.0.0.1"),
            ("osc_path_connected", "/avatar/parameters/hr_connected"),
            ("osc_path_percent", "/avatar/parameters/hr_percent"),
            ("stromno_widget_id", ""),
            ("pico_strap_name", "  "),
        ] {
            assert!(!c.set(key, value), "{key} = {value:?}");
        }
        assert_eq!(c, Config::default());
    }

    #[test]
    fn every_text_key_reaches_its_field() {
        let mut c = Config::default();
        for key in [
            "osc_client_host",
            "osc_path_connected",
            "osc_path_percent",
            "stromno_widget_id",
            "pico_strap_name",
        ] {
            assert!(c.set(key, "x"), "{key}");
        }
        assert_eq!(
            [
                &c.osc_client_host,
                &c.osc_path_connected,
                &c.osc_path_percent,
                &c.stromno_widget_id,
                &c.pico_strap_name
            ],
            ["x"; 5]
        );
    }

    #[test]
    fn unknown_keys_change_nothing() {
        let mut c = Config::default();
        assert!(!c.set("volume", "11"));
        assert_eq!(c, Config::default());
    }

    #[test]
    fn pulsoid_keeps_the_upstream_key() {
        let c = Config::from_json(r#"{"service_type":"pulsoid"}"#);
        assert_eq!(c.service_type, Service::Pulsoid);
        let text = serde_json::to_string(&c).unwrap();
        assert!(text.contains(r#""service_type":"stromno""#));
        assert_eq!(Config::from_json(&text), c);
    }
}
