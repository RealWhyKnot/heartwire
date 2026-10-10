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
fn both_taskbar_frames_are_64_px() {
    let app = app();
    let still = app.get_taskbar_icon();
    app.set_beat(true);
    let beating = app.get_taskbar_icon();
    assert!(still != beating, "the beat swaps the image");
    for icon in [still, beating] {
        let size = icon.size();
        assert_eq!((size.width, size.height), (64, 64));
    }
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

fn same(a: &Frame, b: &Frame) -> bool {
    a.pixels
        .iter()
        .zip(&b.pixels)
        .all(|(p, q)| (p.red, p.green, p.blue) == (q.red, q.green, q.blue))
}

#[test]
fn the_ping_animates_on_screen_and_stops_off_screen() {
    let app = app();
    app.global::<HeartRate>().set_connected(true);
    app.global::<HeartRate>().set_bpm(72);
    advance(0);
    let first = render(&app);
    advance(200);
    let later = render(&app);
    assert!(
        !same(&first, &later),
        "the ping moves while the window is shown"
    );
    app.set_on_screen(false);
    advance(400);
    let hidden = render(&app);
    advance(800);
    assert!(
        same(&hidden, &render(&app)),
        "nothing moves while the window is minimized"
    );
    app.set_on_screen(true);
    advance(1000);
    assert!(!same(&hidden, &render(&app)), "the ping resumes on restore");
}
