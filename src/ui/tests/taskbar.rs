use super::*;

#[test]
fn taskbar_icon_beats_with_the_heart_rate() {
    let app = app();
    let beat = crate::app::taskbar::Beat::new(&app);
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
