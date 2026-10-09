use std::cell::Cell;
use std::rc::Rc;

use i_slint_backend_testing::ElementHandle;
use slint::LogicalPosition;
use slint::platform::{PointerEventButton, WindowEvent};

use super::*;
use crate::ui::Welcome;

fn click(app: &AppWindow, label: &str) {
    let button: Vec<ElementHandle> = all(app, "ChoiceButton::label")
        .into_iter()
        .filter(|e| e.accessible_label().as_deref() == Some(label))
        .collect();
    assert_eq!(button.len(), 1, "one {label} button");
    let r = rect(&button[0]);
    let position = LogicalPosition::new(r.x + r.w / 2.0, r.y + r.h / 2.0);
    let window = app.window();
    window.dispatch_event(WindowEvent::PointerMoved { position });
    window.dispatch_event(WindowEvent::PointerPressed {
        position,
        button: PointerEventButton::Left,
    });
    window.dispatch_event(WindowEvent::PointerReleased {
        position,
        button: PointerEventButton::Left,
    });
}

#[test]
fn welcome_buttons_reach_the_app() {
    let app = app();
    let welcome = app.global::<Welcome>();
    welcome.set_shown(true);
    let imported = Rc::new(Cell::new(0));
    let skipped = Rc::new(Cell::new(0));
    {
        let imported = imported.clone();
        welcome.on_import_hr_osc(move || imported.set(imported.get() + 1));
    }
    {
        let skipped = skipped.clone();
        welcome.on_skip(move || skipped.set(skipped.get() + 1));
    }
    render(&app);
    click(&app, "Import");
    click(&app, "Skip");
    assert_eq!((imported.get(), skipped.get()), (1, 1));
}

#[test]
fn welcome_hides_the_rest_of_the_window() {
    let app = app();
    app.global::<Welcome>().set_shown(true);
    render(&app);
    assert!(all(&app, "NavItem::label").is_empty());
    app.global::<Welcome>().set_shown(false);
    render(&app);
    assert_eq!(all(&app, "NavItem::label").len(), 3);
}
