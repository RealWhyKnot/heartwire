use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::Duration;

use slint::{Timer, TimerMode};

use crate::ui::AppWindow;

pub const FLASH: Duration = Duration::from_millis(120);

thread_local! {
    static BEAT: RefCell<Option<Rc<Beat>>> = const { RefCell::new(None) };
}

pub fn install(window: &AppWindow) {
    BEAT.with(|slot| *slot.borrow_mut() = Some(Beat::new(window)));
}

pub fn update(connected: bool, bpm: u16) {
    BEAT.with(|slot| {
        if let Some(beat) = slot.borrow().as_ref() {
            beat.update(connected, bpm);
        }
    });
}

pub fn interval(bpm: u16) -> Option<Duration> {
    (bpm > 0).then(|| Duration::from_millis(60_000 / u64::from(bpm.clamp(20, 300))))
}

pub struct Beat {
    window: slint::Weak<AppWindow>,
    bpm: Cell<u16>,
    connected: Cell<bool>,
    next: Timer,
    off: Timer,
}

impl Beat {
    pub fn new(window: &AppWindow) -> Rc<Beat> {
        Rc::new(Beat {
            window: slint::ComponentHandle::as_weak(window),
            bpm: Cell::new(0),
            connected: Cell::new(false),
            next: Timer::default(),
            off: Timer::default(),
        })
    }

    pub fn update(self: &Rc<Self>, connected: bool, bpm: u16) {
        self.bpm.set(bpm);
        self.connected.set(connected);
        if connected && interval(bpm).is_some() {
            if !self.next.running() {
                self.tick();
            }
        } else {
            self.next.stop();
            self.off.stop();
            self.set(false);
        }
    }

    fn set(&self, on: bool) {
        if let Some(window) = self.window.upgrade() {
            window.set_beat(on);
        }
    }

    fn tick(self: &Rc<Self>) {
        let Some(every) = interval(self.bpm.get()).filter(|_| self.connected.get()) else {
            return;
        };
        self.set(true);
        let weak: Weak<Beat> = Rc::downgrade(self);
        self.off.start(TimerMode::SingleShot, FLASH, move || {
            if let Some(beat) = weak.upgrade() {
                beat.set(false);
            }
        });
        let weak: Weak<Beat> = Rc::downgrade(self);
        self.next.start(TimerMode::SingleShot, every, move || {
            if let Some(beat) = weak.upgrade() {
                beat.tick();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::interval;
    use std::time::Duration;

    #[test]
    fn interval_follows_bpm() {
        assert_eq!(interval(60), Some(Duration::from_millis(1000)));
        assert_eq!(interval(120), Some(Duration::from_millis(500)));
        assert_eq!(interval(0), None);
        assert_eq!(interval(1000), Some(Duration::from_millis(200)));
        assert_eq!(interval(5), Some(Duration::from_millis(3000)));
    }
}
