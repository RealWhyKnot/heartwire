use std::rc::Rc;
use std::time::{Duration, Instant};

use slint::platform::software_renderer::{
    MinimalSoftwareWindow, PremultipliedRgbaColor, RepaintBufferType,
};
use slint::platform::{Platform, WindowAdapter, WindowEvent};
use slint::{ComponentHandle, PhysicalSize, PlatformError};

use crate::VrPanel;
use crate::steamvr::PanelView;

pub const SCALE: f32 = 2.0;
pub const WIDTH: u32 = 650;
pub const HEIGHT: u32 = 500;

struct PanelPlatform {
    window: Rc<MinimalSoftwareWindow>,
    started: Instant,
}

impl Platform for PanelPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(self.window.clone())
    }

    fn duration_since_start(&self) -> Duration {
        self.started.elapsed()
    }
}

thread_local! {
    static WINDOW: Rc<MinimalSoftwareWindow> = {
        let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
        let _ = slint::platform::set_platform(Box::new(PanelPlatform {
            window: window.clone(),
            started: Instant::now(),
        }));
        window
    };
}

pub struct Panel {
    window: Rc<MinimalSoftwareWindow>,
    ui: VrPanel,
    pixels: Vec<PremultipliedRgbaColor>,
    bytes: Vec<u8>,
    drawn: bool,
}

impl Panel {
    pub fn new() -> Result<Panel, String> {
        let window = WINDOW.with(Rc::clone);
        let ui = VrPanel::new().map_err(|e| e.to_string())?;
        window.dispatch_event(WindowEvent::ScaleFactorChanged {
            scale_factor: SCALE,
        });
        window.set_size(PhysicalSize::new(WIDTH, HEIGHT));
        ui.show().map_err(|e| e.to_string())?;
        Ok(Panel {
            window,
            ui,
            pixels: vec![PremultipliedRgbaColor::default(); (WIDTH * HEIGHT) as usize],
            bytes: vec![0; (WIDTH * HEIGHT * 4) as usize],
            drawn: false,
        })
    }

    #[cfg(windows)]
    pub fn drawn(&self) -> bool {
        self.drawn
    }

    pub fn render(&mut self, view: &PanelView) -> (&[u8], u32, u32) {
        self.ui.set_connected(view.connected);
        self.ui.set_bpm(i32::from(view.bpm));
        self.ui.set_percent_text(view.percent.as_str().into());
        self.ui.set_status(view.status.as_str().into());
        slint::platform::update_timers_and_animations();
        self.window.request_redraw();
        let pixels = &mut self.pixels;
        self.window.draw_if_needed(|renderer| {
            renderer.render(pixels, WIDTH as usize);
        });
        for (out, p) in self
            .bytes
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(&self.pixels)
        {
            *out = [p.red, p.green, p.blue, 255];
        }
        self.drawn = true;
        (&self.bytes, WIDTH, HEIGHT)
    }
}

impl Drop for Panel {
    fn drop(&mut self) {
        let _ = self.ui.hide();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_renders_the_heart_rate_at_double_resolution() {
        let handle = std::thread::spawn(|| {
            let mut panel = Panel::new().unwrap();
            let view = PanelView {
                connected: true,
                bpm: 72,
                percent: "0.36".into(),
                status: "Pico on COM5".into(),
            };
            let (bytes, width, height) = panel.render(&view);
            assert_eq!((width, height), (WIDTH, HEIGHT));
            assert_eq!(bytes.len(), (WIDTH * HEIGHT * 4) as usize);
            let at = |x: u32, y: u32| {
                let i = ((y * WIDTH + x) * 4) as usize;
                [bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]
            };
            assert_eq!(at(2, 2), [0x11, 0x18, 0x27, 255], "background");
            assert_eq!(
                at(WIDTH / 2, 87 * 2),
                [0xf8, 0x71, 0x71, 255],
                "red heart while connected"
            );
            let red_rows = (0..HEIGHT)
                .filter(|&y| (0..WIDTH).any(|x| at(x, y) == [0xf8, 0x71, 0x71, 255]))
                .count();
            assert!(red_rows > 100, "heart and bpm cover {red_rows} rows");
            if let Some(dir) = std::env::var_os("HR_OSC_UI_DUMP") {
                let file =
                    std::fs::File::create(std::path::Path::new(&dir).join("vr-panel.png")).unwrap();
                let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), WIDTH, HEIGHT);
                encoder.set_color(png::ColorType::Rgba);
                encoder.set_depth(png::BitDepth::Eight);
                encoder
                    .write_header()
                    .unwrap()
                    .write_image_data(bytes)
                    .unwrap();
            }
        });
        handle.join().unwrap();
    }
}
