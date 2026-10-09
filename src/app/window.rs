use slint::ComponentHandle;
use slint::winit_030::winit::event::WindowEvent;
use slint::winit_030::{EventResult, WinitWindowAccessor};

use crate::ui::AppWindow;

pub fn on_screen_after(event: &WindowEvent, current: bool) -> bool {
    match event {
        WindowEvent::Resized(size) => size.width > 0 && size.height > 0,
        WindowEvent::Occluded(hidden) => !hidden,
        _ => current,
    }
}

pub fn track_on_screen(window: &AppWindow) {
    let weak = window.as_weak();
    window.window().on_winit_window_event(move |_, event| {
        if let Some(w) = weak.upgrade() {
            let shown = on_screen_after(event, w.get_on_screen());
            if shown != w.get_on_screen() {
                w.set_on_screen(shown);
            }
        }
        EventResult::Propagate
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use slint::winit_030::winit::dpi::PhysicalSize;

    #[test]
    fn minimizing_and_covering_take_the_window_off_screen() {
        let resized = |w, h| WindowEvent::Resized(PhysicalSize::new(w, h));
        assert!(
            !on_screen_after(&resized(0, 0), true),
            "minimized on Windows"
        );
        assert!(on_screen_after(&resized(325, 250), false), "restored");
        assert!(!on_screen_after(&WindowEvent::Occluded(true), true));
        assert!(on_screen_after(&WindowEvent::Occluded(false), false));
        assert!(
            !on_screen_after(&WindowEvent::Focused(true), false),
            "other events keep the state"
        );
        assert!(on_screen_after(&WindowEvent::Focused(false), true));
    }
}
