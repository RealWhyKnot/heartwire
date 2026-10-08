use super::*;

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
    app.global::<Settings>().set_service(3);
    app.global::<Navigation>().set_page(1);
    app.global::<Navigation>().set_tab("parameters".into());
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

    app.global::<Navigation>().set_tab("http".into());
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
fn about_matches_hr_osc() {
    let app = app();
    app.global::<Navigation>().set_page(2);
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
        ink_top(&frame, rect(&one(&app, "AboutPage::title"))) as f32,
        60.0,
        1.0,
    );
    assert_near(
        "version ink top",
        ink_top(&frame, rect(&one(&app, "AboutPage::version"))) as f32,
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
    app.global::<HeartRate>().set_connected(true);
    app.global::<HeartRate>().set_bpm(72);
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
