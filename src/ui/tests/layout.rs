use super::scenes::each_scene;
use super::*;

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
        "BluetoothTab::device-label",
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
