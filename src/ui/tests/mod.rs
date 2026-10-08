mod layout;
mod reference;
mod scenes;
mod taskbar;

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

use crate::ui::{AppWindow, DeviceRow, HeartRate, Navigation, Settings, Updates};

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
    app.global::<Settings>()
        .set_services(ModelRc::new(VecModel::from(services)));
    app.global::<Updates>()
        .set_current_version("2026.10.9.0".into());
    app.global::<Settings>().set_connected_timeout("10".into());
    app.global::<Settings>().set_max_heart_rate("200".into());
    app.global::<Settings>().set_http_port("8080".into());
    app.global::<Settings>().set_osc_host("127.0.0.1".into());
    app.global::<Settings>().set_osc_port("9000".into());
    app.global::<Settings>()
        .set_path_connected("/avatar/parameters/hr_connected".into());
    app.global::<Settings>()
        .set_path_percent("/avatar/parameters/hr_percent".into());
    app.global::<Settings>().set_check_updates(true);
    app.global::<HeartRate>()
        .set_devices(devices(&[("Any heart rate device", "", true)]));
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
    all(app, "SettingsPage::content").first().map(rect)
}

fn on_screen(app: &AppWindow, r: Rect) -> bool {
    match content_column(app) {
        Some(column) if r.x >= column.x - 0.5 => column.contains(&r, 0.5),
        _ => true,
    }
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

fn advance(ms: u64) {
    set_time(ms);
    slint::platform::update_timers_and_animations();
}
