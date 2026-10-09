use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use slint::platform::software_renderer::{
    MinimalSoftwareWindow, PremultipliedRgbaColor, RepaintBufferType,
};
use slint::platform::{Platform, WindowAdapter};
use slint::{ComponentHandle, PhysicalSize, PlatformError};

use super::{Case, Settings, Timing, time};
use crate::ui::{AppWindow, HeartRate, Navigation};

const MS: Duration = Duration::from_millis(1);
const WIDTH: u32 = 325;
const HEIGHT: u32 = 250;

thread_local! {
    static NOW: Cell<Duration> = const { Cell::new(Duration::ZERO) };
}

struct Offscreen {
    window: Rc<MinimalSoftwareWindow>,
}

impl Platform for Offscreen {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(self.window.clone())
    }

    fn duration_since_start(&self) -> Duration {
        NOW.with(Cell::get)
    }
}

struct Screen {
    window: Rc<MinimalSoftwareWindow>,
    app: AppWindow,
    pixels: Vec<PremultipliedRgbaColor>,
}

impl Screen {
    fn new() -> Option<Screen> {
        let window = MinimalSoftwareWindow::new(RepaintBufferType::ReusedBuffer);
        slint::platform::set_platform(Box::new(Offscreen {
            window: window.clone(),
        }))
        .ok()?;
        let app = AppWindow::new().ok()?;
        window.set_size(PhysicalSize::new(WIDTH, HEIGHT));
        app.show().ok()?;
        app.global::<HeartRate>()
            .set_status("Pico: COOSPO HW807".into());
        Some(Screen {
            window,
            app,
            pixels: vec![PremultipliedRgbaColor::default(); (WIDTH * HEIGHT) as usize],
        })
    }

    fn frame(&mut self, full: bool) {
        slint::platform::update_timers_and_animations();
        if full {
            self.window.request_redraw();
        }
        let pixels = &mut self.pixels;
        self.window.draw_if_needed(|renderer| {
            renderer.set_repaint_buffer_type(if full {
                RepaintBufferType::NewBuffer
            } else {
                RepaintBufferType::ReusedBuffer
            });
            renderer.render(pixels, WIDTH as usize);
        });
    }

    fn advance(&self, by: Duration) {
        NOW.with(|now| now.set(now.get() + by));
    }
}

fn window_cases(screen: &Rc<RefCell<Screen>>) -> Vec<Case> {
    let setup = |s: &Screen, page: i32, connected: bool, on_screen: bool| {
        s.app.global::<Navigation>().set_page(page);
        s.app.global::<HeartRate>().set_connected(connected);
        s.app.set_on_screen(on_screen);
    };
    let s = screen.clone();
    let full_home = move || {
        let mut s = s.borrow_mut();
        setup(&s, 0, true, true);
        s.frame(true);
    };
    let s = screen.clone();
    let ping = move || {
        let mut s = s.borrow_mut();
        setup(&s, 0, true, true);
        s.advance(Duration::from_millis(33));
        s.frame(false);
    };
    let s = screen.clone();
    let hidden_ping = move || {
        let mut s = s.borrow_mut();
        setup(&s, 0, true, false);
        s.advance(Duration::from_millis(33));
        s.frame(false);
    };
    let s = screen.clone();
    let mut bpm = 60;
    let bpm_change = move || {
        let mut s = s.borrow_mut();
        setup(&s, 0, true, true);
        bpm = if bpm == 60 { 61 } else { 60 };
        s.app.global::<HeartRate>().set_bpm(bpm);
        s.frame(false);
    };
    let s = screen.clone();
    let settings = move || {
        let mut s = s.borrow_mut();
        setup(&s, 1, true, true);
        s.frame(true);
    };
    vec![
        Case::new("ui", "main window full frame", 20 * MS, full_home),
        Case::new("ui", "ping animation frame", 5 * MS, ping),
        Case::new("ui", "ping frame while minimized", MS, hidden_ping),
        Case::new("ui", "bpm change frame", 5 * MS, bpm_change),
        Case::new("ui", "settings page full frame", 20 * MS, settings),
    ]
}

fn window_timings(settings: Settings) -> Vec<Timing> {
    let Some(screen) = Screen::new() else {
        return Vec::new();
    };
    let screen = Rc::new(RefCell::new(screen));
    window_cases(&screen)
        .iter_mut()
        .map(|case| time(case, settings))
        .collect()
}

#[cfg(any(windows, test))]
fn panel_timings(settings: Settings) -> Vec<Timing> {
    use crate::steamvr::PanelView;
    use crate::steamvr::panel::Panel;
    let view = |bpm: u16| PanelView {
        connected: true,
        bpm,
        percent: format!("{:.2}", f32::from(bpm) / 200.0),
        status: "Pico: COOSPO HW807".into(),
    };
    let Ok(panel) = Panel::new() else {
        return Vec::new();
    };
    let panel = Rc::new(RefCell::new(panel));
    let mut bpm = 60;
    let changed = {
        let panel = panel.clone();
        move || {
            bpm = if bpm == 60 { 61 } else { 60 };
            std::hint::black_box(panel.borrow_mut().render(&view(bpm)).0.len());
        }
    };
    let mut cases = vec![Case::new(
        "steamvr",
        "VR panel frame after a bpm change",
        10 * MS,
        changed,
    )];
    let mut timings: Vec<Timing> = cases.iter_mut().map(|c| time(c, settings)).collect();
    drop(cases);
    drop(panel);
    let mut first = Case::new(
        "steamvr",
        "VR panel create and first frame",
        100 * MS,
        move || {
            if let Ok(mut panel) = Panel::new() {
                std::hint::black_box(panel.render(&view(72)).0.len());
            }
        },
    );
    timings.push(time(&mut first, settings));
    timings
}

pub fn timings(settings: Settings) -> Vec<Timing> {
    let window = std::thread::spawn(move || window_timings(settings));
    #[cfg(any(windows, test))]
    let panel = std::thread::spawn(move || panel_timings(settings));
    let mut out = window.join().unwrap_or_default();
    #[cfg(any(windows, test))]
    out.extend(panel.join().unwrap_or_default());
    out
}
