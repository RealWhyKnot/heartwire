use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use i_slint_backend_testing::{ElementHandle, ElementRoot};
use slint::platform::software_renderer::{
    MinimalSoftwareWindow, PremultipliedRgbaColor as Pixel, RepaintBufferType,
};
use slint::platform::{Platform, WindowAdapter};
use slint::{ComponentHandle, ModelRc, PhysicalSize, PlatformError, SharedString, VecModel};

use crate::{AppWindow, DeviceRow};

const WIDTH: u32 = 325;
const HEIGHT: u32 = 250;
const INK: u8 = 36;

thread_local! {
    static WINDOW: Rc<MinimalSoftwareWindow> = {
        let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
        let _ = slint::platform::set_platform(Box::new(TestPlatform { window: window.clone() }));
        window
    };
    static NOW: Cell<Duration> = const { Cell::new(Duration::ZERO) };
    static BASE: Cell<Duration> = const { Cell::new(Duration::ZERO) };
}

struct TestPlatform {
    window: Rc<MinimalSoftwareWindow>,
}

impl Platform for TestPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(self.window.clone())
    }

    fn duration_since_start(&self) -> Duration {
        NOW.with(Cell::get)
    }
}

struct Frame {
    pixels: Vec<Pixel>,
}

impl Frame {
    fn at(&self, x: i32, y: i32) -> [u8; 3] {
        let p = self.pixels[(y as u32 * WIDTH + x as u32) as usize];
        [p.red, p.green, p.blue]
    }

    fn save(&self, path: &std::path::Path) {
        let file = std::fs::File::create(path).unwrap();
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), WIDTH, HEIGHT);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        let bytes: Vec<u8> = self
            .pixels
            .iter()
            .flat_map(|p| [p.red, p.green, p.blue])
            .collect();
        writer.write_image_data(&bytes).unwrap();
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Rect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl Rect {
    fn right(&self) -> f32 {
        self.x + self.w
    }
    fn bottom(&self) -> f32 {
        self.y + self.h
    }
    fn center_y(&self) -> f32 {
        self.y + self.h / 2.0
    }
    fn contains(&self, other: &Rect, slack: f32) -> bool {
        other.x >= self.x - slack
            && other.y >= self.y - slack
            && other.right() <= self.right() + slack
            && other.bottom() <= self.bottom() + slack
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Ink {
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
}

impl Ink {
    fn center_y(&self) -> f32 {
        (self.y0 + self.y1) as f32 / 2.0
    }
    fn overlaps(&self, other: &Ink) -> bool {
        self.x0 < other.x1 && other.x0 < self.x1 && self.y0 < other.y1 && other.y0 < self.y1
    }
}

fn app() -> AppWindow {
    let window = WINDOW.with(Rc::clone);
    let base = NOW.with(Cell::get) + Duration::from_secs(10);
    BASE.with(|b| b.set(base));
    set_time(0);
    let app = AppWindow::new().unwrap();
    window.set_size(PhysicalSize::new(WIDTH, HEIGHT));
    app.show().unwrap();
    let services: Vec<SharedString> = crate::config::Service::ALL
        .iter()
        .map(|s| s.label().into())
        .collect();
    app.set_services(ModelRc::new(VecModel::from(services)));
    app.set_version("2026.10.9.0".into());
    app.set_connected_timeout("10".into());
    app.set_max_heart_rate("200".into());
    app.set_http_port("8080".into());
    app.set_osc_host("127.0.0.1".into());
    app.set_osc_port("9000".into());
    app.set_path_connected("/avatar/parameters/hr_connected".into());
    app.set_path_percent("/avatar/parameters/hr_percent".into());
    app.set_check_updates(true);
    app.set_devices(devices(&[("Any heart rate device", "", true)]));
    app
}

fn set_time(ms: u64) {
    let base = BASE.with(Cell::get);
    NOW.with(|n| n.set(n.get().max(base + Duration::from_millis(ms))));
}

fn render(_app: &AppWindow) -> Frame {
    let window = WINDOW.with(Rc::clone);
    slint::platform::update_timers_and_animations();
    window.request_redraw();
    let mut pixels = vec![Pixel::default(); (WIDTH * HEIGHT) as usize];
    window.draw_if_needed(|renderer| {
        renderer.render(&mut pixels, WIDTH as usize);
    });
    Frame { pixels }
}

fn rect(e: &ElementHandle) -> Rect {
    let p = e.absolute_position();
    let s = e.size();
    Rect {
        x: p.x,
        y: p.y,
        w: s.width,
        h: s.height,
    }
}

fn all(app: &AppWindow, id: &str) -> Vec<ElementHandle> {
    ElementHandle::find_by_element_id(app, id).collect()
}

fn one(app: &AppWindow, id: &str) -> ElementHandle {
    let mut found = all(app, id);
    assert_eq!(found.len(), 1, "{id} matched {} elements", found.len());
    found.remove(0)
}

fn texts(app: &AppWindow) -> Vec<ElementHandle> {
    app.root_element()
        .query_descendants()
        .match_predicate(|e| {
            matches!(
                e.type_name().as_deref(),
                Some("Text") | Some("TextInput") | Some("Label")
            )
        })
        .find_all()
}

fn devices(rows: &[(&str, &str, bool)]) -> ModelRc<DeviceRow> {
    ModelRc::new(VecModel::from(
        rows.iter()
            .map(|(name, address, selected)| DeviceRow {
                name: (*name).into(),
                address: (*address).into(),
                selected: *selected,
            })
            .collect::<Vec<_>>(),
    ))
}

fn distance(a: [u8; 3], b: [u8; 3]) -> u8 {
    (0..3).map(|i| a[i].abs_diff(b[i])).max().unwrap_or(0)
}

fn clamp_x(v: f32) -> i32 {
    (v as i32).clamp(0, WIDTH as i32)
}

fn clamp_y(v: f32) -> i32 {
    (v as i32).clamp(0, HEIGHT as i32)
}

fn ink_in(frame: &Frame, r: Rect, pad: f32) -> Option<Ink> {
    ink_excluding(frame, r, pad, &[])
}

fn ink_excluding(frame: &Frame, r: Rect, pad: f32, others: &[Rect]) -> Option<Ink> {
    let x0 = clamp_x((r.x - pad).floor());
    let y0 = clamp_y((r.y - pad).floor());
    let x1 = clamp_x((r.right() + pad).ceil());
    let y1 = clamp_y((r.bottom() + pad).ceil());
    if x1 - x0 < 2 || y1 - y0 < 2 {
        return None;
    }
    let mut counts: HashMap<[u8; 3], usize> = HashMap::new();
    for x in x0..x1 {
        *counts.entry(frame.at(x, y0)).or_default() += 1;
        *counts.entry(frame.at(x, y1 - 1)).or_default() += 1;
    }
    for y in y0..y1 {
        *counts.entry(frame.at(x0, y)).or_default() += 1;
        *counts.entry(frame.at(x1 - 1, y)).or_default() += 1;
    }
    let background = counts.into_iter().max_by_key(|(_, n)| *n)?.0;
    let inside = |x: i32, y: i32| {
        let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
        fx >= r.x && fx < r.right() && fy >= r.y && fy < r.bottom()
    };
    let foreign = |x: i32, y: i32| {
        let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
        !inside(x, y)
            && others
                .iter()
                .any(|o| fx >= o.x && fx < o.right() && fy >= o.y && fy < o.bottom())
    };
    let mut ink: Option<Ink> = None;
    for y in y0..y1 {
        for x in x0..x1 {
            if foreign(x, y) || distance(frame.at(x, y), background) <= INK {
                continue;
            }
            let i = ink.get_or_insert(Ink {
                x0: x,
                y0: y,
                x1: x + 1,
                y1: y + 1,
            });
            i.x0 = i.x0.min(x);
            i.y0 = i.y0.min(y);
            i.x1 = i.x1.max(x + 1);
            i.y1 = i.y1.max(y + 1);
        }
    }
    ink
}

fn content_column(app: &AppWindow) -> Option<Rect> {
    all(app, "AppWindow::content").first().map(rect)
}

fn on_screen(app: &AppWindow, r: Rect) -> bool {
    match content_column(app) {
        Some(column) if r.x >= column.x - 0.5 => column.contains(&r, 0.5),
        _ => true,
    }
}

type Setup = Box<dyn Fn(&AppWindow)>;

fn scenes() -> Vec<(String, Setup)> {
    let mut out: Vec<(String, Setup)> = Vec::new();
    let mut add = |name: &str, setup: Setup| out.push((name.to_owned(), setup));

    add(
        "home-idle",
        Box::new(|a| a.set_status("Waiting for a Pico".into())),
    );
    add(
        "home-streaming",
        Box::new(|a| {
            a.set_connected(true);
            a.set_bpm(188);
            a.set_percent_text("0.94".into());
            a.set_status("Bluetooth: COOSPO HW807".into());
            set_time(900);
        }),
    );
    add(
        "home-long-status",
        Box::new(|a| {
            a.set_status(
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
                    a.set_service(service);
                    a.set_page(1);
                    a.set_tab(tab.as_str().into());
                }),
            );
        }
    }
    add(
        "bluetooth-devices",
        Box::new(|a| {
            a.set_service(1);
            a.set_page(1);
            a.set_tab("bluetooth".into());
            a.set_status("Searching for a heart rate device".into());
            a.set_devices(devices(&[
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
            a.set_service(1);
            a.set_page(1);
            a.set_tab("bluetooth".into());
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
            a.set_devices(devices(&rows));
        }),
    );
    add(
        "pico-notice",
        Box::new(|a| {
            a.set_service(2);
            a.set_page(1);
            a.set_tab("pico".into());
            a.set_status("Pico is searching for a strap".into());
            a.set_pico_strap("COOSPO".into());
            a.set_notice(
                "Install failed: the board did not answer; is MicroPython installed?".into(),
            );
        }),
    );
    add(
        "general-unchecked",
        Box::new(|a| {
            a.set_page(1);
            a.set_tab("general".into());
            a.set_autostart(false);
            a.set_check_updates(false);
        }),
    );
    add(
        "general-checked",
        Box::new(|a| {
            a.set_page(1);
            a.set_tab("general".into());
            a.set_autostart(true);
            a.set_check_updates(true);
        }),
    );
    add(
        "parameters-empty",
        Box::new(|a| {
            a.set_page(1);
            a.set_tab("parameters".into());
            a.set_path_connected("".into());
            a.set_path_percent("".into());
            a.set_max_heart_rate("".into());
        }),
    );
    add("about", Box::new(|a| a.set_page(2)));
    add(
        "about-beta",
        Box::new(|a| {
            a.set_page(2);
            a.set_version("2026.12.31.10-beta".into());
        }),
    );
    add(
        "update",
        Box::new(|a| {
            a.set_update_version("2026.10.12.0".into());
            a.set_update_shown(true);
        }),
    );
    add(
        "update-busy",
        Box::new(|a| {
            a.set_update_version("2026.10.12.0".into());
            a.set_update_shown(true);
            a.set_update_busy(true);
            a.set_update_state("Downloading 42%".into());
        }),
    );
    add(
        "update-failed",
        Box::new(|a| {
            a.set_update_version("2026.10.12.0".into());
            a.set_update_shown(true);
            a.set_update_state(
                "Update failed: hr-osc-rust-2026.10.12.0-win-x64.zip sha256 does not match".into(),
            );
        }),
    );
    out
}

fn each_scene(mut check: impl FnMut(&str, &AppWindow, &Frame)) {
    for (name, setup) in scenes() {
        let app = app();
        setup(&app);
        let frame = render(&app);
        if let Some(dir) = std::env::var_os("HR_OSC_UI_DUMP") {
            let dir = std::path::PathBuf::from(dir);
            std::fs::create_dir_all(&dir).unwrap();
            frame.save(&dir.join(format!("{name}.png")));
        }
        check(&name, &app, &frame);
    }
}

#[test]
fn every_element_stays_inside_the_window() {
    let window = Rect {
        x: 0.0,
        y: 0.0,
        w: WIDTH as f32,
        h: HEIGHT as f32,
    };
    each_scene(|name, app, _| {
        for e in app.root_element().query_descendants().find_all() {
            let r = rect(&e);
            if r.w <= 0.0 || r.h <= 0.0 || !on_screen(app, r) {
                continue;
            }
            assert!(
                window.contains(&r, 0.5),
                "{name}: {:?} {:?} sits at {r:?}",
                e.id(),
                e.type_name()
            );
        }
    });
}

#[test]
fn text_stays_inside_its_box() {
    each_scene(|name, app, frame| {
        let all_texts = texts(app);
        let rects: Vec<Rect> = all_texts.iter().map(rect).collect();
        for e in &all_texts {
            let r = rect(e);
            if e.computed_opacity() < 0.99 || !on_screen(app, r) {
                continue;
            }
            let others: Vec<Rect> = rects.iter().copied().filter(|o| *o != r).collect();
            let Some(ink) = ink_excluding(frame, r, 3.0, &others) else {
                continue;
            };
            let bounds = Rect {
                x: r.x - 1.0,
                y: r.y - 2.0,
                w: r.w + 2.0,
                h: r.h + 4.0,
            };
            let ink_rect = Rect {
                x: ink.x0 as f32,
                y: ink.y0 as f32,
                w: (ink.x1 - ink.x0) as f32,
                h: (ink.y1 - ink.y0) as f32,
            };
            assert!(
                bounds.contains(&ink_rect, 0.0),
                "{name}: {:?} ({:?}) draws {ink:?} outside its box {r:?}",
                e.id(),
                e.accessible_label()
            );
        }
    });
}

#[test]
fn visible_text_is_drawn() {
    each_scene(|name, app, frame| {
        for e in texts(app) {
            let r = rect(&e);
            let label = e.accessible_label().unwrap_or_default();
            if label.trim().is_empty() || e.computed_opacity() < 0.99 || !on_screen(app, r) {
                continue;
            }
            assert!(
                ink_in(frame, r, 0.0).is_some(),
                "{name}: {:?} {label:?} at {r:?} draws nothing",
                e.id()
            );
        }
    });
}

#[test]
fn text_never_overlaps_other_text() {
    each_scene(|name, app, frame| {
        let inks: Vec<(String, Ink)> = texts(app)
            .iter()
            .filter(|e| e.computed_opacity() >= 0.99 && on_screen(app, rect(e)))
            .filter_map(|e| {
                let label = e.accessible_label().unwrap_or_default().to_string();
                if label.trim().is_empty() {
                    return None;
                }
                ink_in(frame, rect(e), 0.0).map(|i| (label, i))
            })
            .collect();
        for (i, (a, ia)) in inks.iter().enumerate() {
            for (b, ib) in &inks[i + 1..] {
                assert!(
                    !ia.overlaps(ib),
                    "{name}: {a:?} {ia:?} overlaps {b:?} {ib:?}"
                );
            }
        }
    });
}

#[test]
fn controls_center_their_text() {
    let ids = [
        "SmallButton::label",
        "Field::input",
        "Field::placeholder-label",
        "Select::value",
        "Check::caption",
        "NavItem::label",
        "AppWindow::device-label",
    ];
    each_scene(|name, app, frame| {
        for id in ids {
            for e in all(app, id) {
                let r = rect(&e);
                if !on_screen(app, r) {
                    continue;
                }
                let Some(ink) = ink_in(frame, r, 0.0) else {
                    continue;
                };
                let offset = ink.center_y() - r.center_y();
                assert!(
                    offset.abs() <= 2.0,
                    "{name}: {id} {:?} ink {ink:?} is {offset:+.1}px off centre of {r:?}",
                    e.accessible_label()
                );
                assert!(
                    ink.y0 as f32 >= r.y - 0.5 && ink.y1 as f32 <= r.bottom() + 0.5,
                    "{name}: {id} {:?} ink {ink:?} is clipped by {r:?}",
                    e.accessible_label()
                );
            }
        }
    });
}

fn label_rect(app: &AppWindow, id: &str, text: &str) -> Rect {
    let found: Vec<ElementHandle> = all(app, id)
        .into_iter()
        .filter(|e| e.accessible_label().as_deref() == Some(text))
        .collect();
    assert_eq!(found.len(), 1, "{id} {text:?} matched {}", found.len());
    rect(&found[0])
}

fn ink_top(frame: &Frame, r: Rect) -> i32 {
    ink_in(frame, r, 0.0).expect("text is drawn").y0
}

fn assert_near(what: &str, actual: f32, expected: f32, slack: f32) {
    assert!(
        (actual - expected).abs() <= slack,
        "{what}: {actual} but hr-osc has {expected}"
    );
}

#[test]
fn nav_matches_hr_osc() {
    let app = app();
    let frame = render(&app);
    for (text, x) in [("Home", 9), ("Settings", 63), ("About", 134)] {
        let r = label_rect(&app, "NavItem::label", text);
        let ink = ink_in(&frame, r, 0.0).unwrap();
        assert_near(&format!("{text} ink left"), ink.x0 as f32, x as f32, 1.0);
        assert_near(&format!("{text} ink top"), ink.y0 as f32, 14.0, 1.0);
    }
    let underline = ink_in(
        &frame,
        Rect {
            x: 0.0,
            y: 31.0,
            w: 120.0,
            h: 4.0,
        },
        0.0,
    )
    .unwrap();
    assert_eq!((underline.y0, underline.y1), (32, 34));
    assert_near("underline left", underline.x0 as f32, 8.0, 0.0);
    assert_near("underline right", underline.x1 as f32, 51.0, 1.0);
}

#[test]
fn home_matches_hr_osc() {
    let app = app();
    let frame = render(&app);
    let heart = rect(&one(&app, "Home::heart"));
    assert_eq!(
        heart,
        Rect {
            x: 134.5,
            y: 59.0,
            w: 56.0,
            h: 56.0
        }
    );
    for (id, top) in [
        ("Home::bpm-label", 133.0),
        ("Home::percent-label", 159.0),
        ("Home::status-title", 200.0),
        ("Home::status-value", 219.0),
    ] {
        let r = rect(&one(&app, id));
        assert_near(
            &format!("{id} ink top"),
            ink_top(&frame, r) as f32,
            top,
            1.0,
        );
    }
}

#[test]
fn settings_match_hr_osc() {
    let app = app();
    app.set_service(3);
    app.set_page(1);
    app.set_tab("parameters".into());
    render(&app);
    let buttons: Vec<Rect> = all(&app, "SmallButton::label")
        .iter()
        .map(|e| {
            let r = rect(e);
            Rect {
                x: r.x - 8.0,
                y: r.y,
                w: r.w + 16.0,
                h: r.h,
            }
        })
        .collect();
    let expected: Vec<Rect> = [46.0, 82.0, 118.0, 154.0]
        .iter()
        .map(|&y| Rect {
            x: 8.0,
            y,
            w: 98.0,
            h: 28.0,
        })
        .collect();
    assert_eq!(buttons, expected);
    let fields: Vec<Rect> = all(&app, "Field::input")
        .iter()
        .map(|e| {
            let r = rect(e);
            Rect {
                x: r.x - 8.0,
                y: r.y,
                w: r.w + 16.0,
                h: r.h,
            }
        })
        .collect();
    let expected: Vec<Rect> = [66.0, 128.0, 190.0]
        .iter()
        .map(|&y| Rect {
            x: 114.0,
            y,
            w: 180.0,
            h: 24.0,
        })
        .collect();
    assert_eq!(fields, expected);

    app.set_tab("http".into());
    render(&app);
    let restart = label_rect(&app, "SmallButton::label", "Restart Server");
    assert_eq!(
        Rect {
            x: restart.x - 8.0,
            w: restart.w + 16.0,
            ..restart
        },
        Rect {
            x: 114.0,
            y: 108.0,
            w: 180.0,
            h: 28.0
        }
    );
}

#[test]
fn every_tab_keeps_the_same_rhythm() {
    each_scene(|name, app, _| {
        if !name.starts_with("settings-") {
            return;
        }
        let mut found: Vec<Rect> = app
            .root_element()
            .query_descendants()
            .match_predicate(|e| {
                matches!(
                    e.type_name().as_deref(),
                    Some("Label")
                        | Some("Field")
                        | Some("Select")
                        | Some("SmallButton")
                        | Some("Check")
                        | Some("Text")
                        | Some("Rectangle")
                )
            })
            .find_all()
            .iter()
            .map(rect)
            .filter(|r| r.x == 114.0 && r.h > 2.0 && r.y >= 40.0)
            .collect();
        found.sort_by(|a, b| a.y.total_cmp(&b.y).then(b.h.total_cmp(&a.h)));
        let mut rows: Vec<Rect> = Vec::new();
        for r in found {
            match rows.last_mut() {
                Some(last) if r.y < last.bottom() => {}
                Some(last) if r.y - last.bottom() <= 2.0 => {
                    last.h = r.bottom() - last.y;
                }
                _ => rows.push(r),
            }
        }
        assert!(!rows.is_empty(), "{name}: no settings rows found");
        assert_eq!(rows[0].y, 46.0, "{name}: first row starts at {}", rows[0].y);
        for pair in rows.windows(2) {
            let gap = pair[1].y - pair[0].bottom();
            assert!(
                gap == 8.0 || gap == 18.0,
                "{name}: {gap}px between {:?} and {:?}",
                pair[0],
                pair[1]
            );
        }
        for r in &rows {
            assert_eq!(
                (r.x, r.w),
                (114.0, 180.0),
                "{name}: row {r:?} is off the column"
            );
        }
    });
}

#[test]
fn about_matches_hr_osc() {
    let app = app();
    app.set_page(2);
    let frame = render(&app);
    let icon = app
        .root_element()
        .query_descendants()
        .match_type_name("Image")
        .find_first()
        .map(|e| rect(&e))
        .unwrap();
    assert_eq!(
        icon,
        Rect {
            x: 24.0,
            y: 54.0,
            w: 48.0,
            h: 48.0
        }
    );
    assert_near(
        "title ink top",
        ink_top(&frame, rect(&one(&app, "AppWindow::about-title"))) as f32,
        60.0,
        1.0,
    );
    assert_near(
        "version ink top",
        ink_top(&frame, rect(&one(&app, "AppWindow::about-version"))) as f32,
        90.0,
        1.0,
    );
    let links: Vec<i32> = all(&app, "Link::label")
        .iter()
        .map(|e| ink_top(&frame, rect(e)))
        .collect();
    assert_eq!(links.len(), 3);
    for (actual, expected) in links.iter().zip([156, 188, 208]) {
        assert_near("link ink top", *actual as f32, expected as f32, 1.0);
    }
}

#[test]
fn ping_animation_stays_behind_the_numbers() {
    let app = app();
    app.set_connected(true);
    app.set_bpm(72);
    let bpm = rect(&one(&app, "Home::bpm-label"));
    for ms in (0..1000).step_by(50) {
        set_time(ms);
        let frame = render(&app);
        let ink = ink_in(&frame, bpm, 0.0).expect("bpm is drawn");
        assert!(
            ink.y1 as f32 <= bpm.bottom() + 2.0,
            "at {ms}ms the bpm ink is {ink:?}"
        );
    }
}

fn advance(ms: u64) {
    set_time(ms);
    slint::platform::update_timers_and_animations();
}

#[test]
fn taskbar_icon_beats_with_the_heart_rate() {
    let app = app();
    let beat = crate::beat::Beat::new(&app);
    beat.update(true, 60);
    assert!(app.get_beat(), "the first beat flashes at once");
    advance(130);
    assert!(!app.get_beat(), "the flash ends after 120ms");
    advance(1005);
    assert!(app.get_beat(), "60 bpm beats again after one second");
    advance(1130);
    assert!(!app.get_beat());
    beat.update(true, 120);
    advance(2005);
    assert!(app.get_beat(), "the beat already scheduled still lands");
    advance(2130);
    assert!(!app.get_beat());
    advance(2505);
    assert!(app.get_beat(), "120 bpm beats every 500ms");
    beat.update(false, 120);
    assert!(!app.get_beat(), "losing the signal stops the flash");
    advance(4000);
    assert!(!app.get_beat(), "no beats while disconnected");
}

#[test]
fn beat_frame_keeps_the_window_layout() {
    let app = app();
    let still = render(&app);
    app.set_beat(true);
    let beating = render(&app);
    assert!(
        still
            .pixels
            .iter()
            .zip(&beating.pixels)
            .all(|(a, b)| (a.red, a.green, a.blue) == (b.red, b.green, b.blue)),
        "the beat only changes the taskbar icon"
    );
}
