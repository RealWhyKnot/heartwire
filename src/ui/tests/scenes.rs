use super::*;

pub(super) type Setup = Box<dyn Fn(&AppWindow)>;

pub(super) fn scenes() -> Vec<(String, Setup)> {
    let mut out: Vec<(String, Setup)> = Vec::new();
    let mut add = |name: &str, setup: Setup| out.push((name.to_owned(), setup));

    add(
        "home-idle",
        Box::new(|a| {
            a.global::<HeartRate>()
                .set_status("Waiting for a Pico".into())
        }),
    );
    add(
        "home-streaming",
        Box::new(|a| {
            a.global::<HeartRate>().set_connected(true);
            a.global::<HeartRate>().set_bpm(188);
            a.global::<HeartRate>().set_percent_text("0.94".into());
            a.global::<HeartRate>()
                .set_status("Bluetooth: COOSPO HW807".into());
            set_time(900);
        }),
    );
    add(
        "home-long-status",
        Box::new(|a| {
            a.global::<HeartRate>().set_status(
                "The Pico is silent. Install the firmware in Settings > Pico and wait".into(),
            )
        }),
    );

    let tabs: [(i32, &[&str]); 5] = [
        (0, &["general", "bluetooth", "pico", "osc", "parameters"]),
        (1, &["general", "bluetooth", "osc", "parameters"]),
        (2, &["general", "pico", "osc", "parameters"]),
        (3, &["general", "http", "osc", "parameters"]),
        (4, &["general", "pulsoid", "osc", "parameters"]),
    ];
    for (service, list) in tabs {
        for tab in list {
            let tab = tab.to_string();
            add(
                &format!("settings-{service}-{tab}"),
                Box::new(move |a| {
                    a.global::<Settings>().set_service(service);
                    a.global::<Navigation>().set_page(1);
                    a.global::<Navigation>().set_tab(tab.as_str().into());
                }),
            );
        }
    }
    add(
        "bluetooth-devices",
        Box::new(|a| {
            a.global::<Settings>().set_service(1);
            a.global::<Navigation>().set_page(1);
            a.global::<Navigation>().set_tab("bluetooth".into());
            a.global::<HeartRate>()
                .set_status("Searching for a heart rate device".into());
            a.global::<HeartRate>().set_devices(devices(&[
                ("Any heart rate device", "", false),
                (
                    "COOSPO HW807 (C0:FF:EE:00:11:22)",
                    "C0:FF:EE:00:11:22",
                    true,
                ),
                (
                    "Polar H10 A1B2C3D4 with a very long advertised name (C1:22:33:44:55:66)",
                    "C1:22:33:44:55:66",
                    false,
                ),
                (
                    "Forerunner 265 (D0:11:22:33:44:55)",
                    "D0:11:22:33:44:55",
                    false,
                ),
            ]));
        }),
    );
    add(
        "bluetooth-many-devices",
        Box::new(|a| {
            a.global::<Settings>().set_service(1);
            a.global::<Navigation>().set_page(1);
            a.global::<Navigation>().set_tab("bluetooth".into());
            let names: Vec<(String, String)> = (0..14)
                .map(|i| {
                    (
                        format!("Strap {i} (AA:BB:CC:DD:EE:{i:02X})"),
                        format!("AA:BB:CC:DD:EE:{i:02X}"),
                    )
                })
                .collect();
            let rows: Vec<(&str, &str, bool)> = names
                .iter()
                .map(|(n, a)| (n.as_str(), a.as_str(), false))
                .collect();
            a.global::<HeartRate>().set_devices(devices(&rows));
        }),
    );
    add(
        "pico-notice",
        Box::new(|a| {
            a.global::<Settings>().set_service(2);
            a.global::<Navigation>().set_page(1);
            a.global::<Navigation>().set_tab("pico".into());
            a.global::<HeartRate>()
                .set_status("Pico is searching for a strap".into());
            a.global::<Settings>().set_pico_strap("COOSPO".into());
            a.global::<HeartRate>().set_notice(
                "Install failed: the board did not answer; is MicroPython installed?".into(),
            );
        }),
    );
    add(
        "general-unchecked",
        Box::new(|a| {
            a.global::<Navigation>().set_page(1);
            a.global::<Navigation>().set_tab("general".into());
            a.global::<Settings>().set_autostart(false);
            a.global::<Settings>().set_check_updates(false);
        }),
    );
    add(
        "general-checked",
        Box::new(|a| {
            a.global::<Navigation>().set_page(1);
            a.global::<Navigation>().set_tab("general".into());
            a.global::<Settings>().set_autostart(true);
            a.global::<Settings>().set_check_updates(true);
        }),
    );
    add(
        "parameters-empty",
        Box::new(|a| {
            a.global::<Navigation>().set_page(1);
            a.global::<Navigation>().set_tab("parameters".into());
            a.global::<Settings>().set_path_connected("".into());
            a.global::<Settings>().set_path_percent("".into());
            a.global::<Settings>().set_max_heart_rate("".into());
        }),
    );
    add("about", Box::new(|a| a.global::<Navigation>().set_page(2)));
    add(
        "about-beta",
        Box::new(|a| {
            a.global::<Navigation>().set_page(2);
            a.global::<Updates>()
                .set_current_version("2026.12.31.10-beta".into());
        }),
    );
    add(
        "update",
        Box::new(|a| {
            a.global::<Updates>().set_version("2026.10.12.0".into());
            a.global::<Updates>().set_shown(true);
        }),
    );
    add(
        "update-busy",
        Box::new(|a| {
            a.global::<Updates>().set_version("2026.10.12.0".into());
            a.global::<Updates>().set_shown(true);
            a.global::<Updates>().set_busy(true);
            a.global::<Updates>().set_state("Downloading 42%".into());
        }),
    );
    add(
        "update-failed",
        Box::new(|a| {
            a.global::<Updates>().set_version("2026.10.12.0".into());
            a.global::<Updates>().set_shown(true);
            a.global::<Updates>().set_state(
                "Update failed: heartwire-2026.10.12.0-win-x64.zip sha256 does not match".into(),
            );
        }),
    );
    out
}

pub(super) fn each_scene(mut check: impl FnMut(&str, &AppWindow, &Frame)) {
    for (name, setup) in scenes() {
        let app = app();
        setup(&app);
        let frame = render(&app);
        if let Some(dir) = std::env::var_os("HEARTWIRE_UI_DUMP") {
            let dir = std::path::PathBuf::from(dir);
            std::fs::create_dir_all(&dir).unwrap();
            frame.save(&dir.join(format!("{name}.png")));
        }
        check(&name, &app, &frame);
    }
}
