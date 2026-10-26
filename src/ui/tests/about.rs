use std::cell::RefCell;
use std::rc::Rc;

use super::*;

const REPO: &str = "https://github.com/RealWhyKnot/heartwire";

fn slug(heading: &str) -> String {
    heading
        .trim()
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-' || *c == '_')
        .map(|c| if c == ' ' { '-' } else { c })
        .collect()
}

fn readme_anchors() -> Vec<String> {
    include_str!("../../../README.md")
        .lines()
        .filter(|line| line.starts_with('#'))
        .filter_map(|line| line.trim_start_matches('#').strip_prefix(' '))
        .map(slug)
        .collect()
}

#[test]
fn about_links_open_pages_that_exist() {
    let app = app();
    app.global::<Navigation>().set_page(2);
    let opened = Rc::new(RefCell::new(Vec::new()));
    {
        let opened = opened.clone();
        app.global::<Navigation>()
            .on_open_link(move |url| opened.borrow_mut().push(url.to_string()));
    }
    render(&app);
    let links = all(&app, "Link::label");
    assert_eq!(links.len(), 3);
    for link in &links {
        press(&app, link);
    }
    let opened = opened.borrow();
    assert_eq!(opened.len(), 3, "{opened:?}");
    let anchors = readme_anchors();
    let prefix = format!("{REPO}#");
    let linked: Vec<&str> = opened
        .iter()
        .filter_map(|url| url.strip_prefix(prefix.as_str()))
        .collect();
    assert!(!linked.is_empty(), "{opened:?}");
    for anchor in linked {
        assert!(
            anchors.iter().any(|a| a == anchor),
            "#{anchor} not in {anchors:?}"
        );
    }
}

#[test]
fn readme_anchors_follow_github_rules() {
    assert_eq!(slug("Pico W"), "pico-w");
    assert_eq!(slug("SteamVR and start-up"), "steamvr-and-start-up");
    assert_eq!(slug("Pulsoid / Stromno"), "pulsoid--stromno");
}
