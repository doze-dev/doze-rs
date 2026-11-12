use super::*;
use crate::assets::Assets;
use crate::fixtures;

fn render(chrome: &Chrome, active: Rail, regions: &[&'static str], prefix: &str) -> String {
    let base = Base::new(prefix);
    let assets = Assets::build(&base);
    let page = Page {
        base: &base,
        assets: &assets.urls,
        chrome,
        active,
        crumbs: vec![
            Crumb {
                label: "Home".into(),
                href: Some(base.url("/")),
            },
            Crumb {
                label: "Connect an app".into(),
                href: None,
            },
        ],
        title: "Connect an app",
        regions,
    };
    document(&page, html! { h1 { "Connect an app" } }).into_string()
}

#[test]
fn the_document_is_html_with_a_language_a_title_and_one_main() {
    let s = render(&fixtures::chrome(), Rail::Home, &[], "/");
    assert!(
        s.starts_with("<!DOCTYPE html><html lang=\"en\">"),
        "{}",
        &s[..60]
    );
    assert!(s.contains("<title>Connect an app · doze-kafka</title>"));
    assert_eq!(s.matches("<main ").count(), 1);
    assert_eq!(s.matches("<h1>").count(), 1);
    assert!(s.contains(r##"<a class="skip" href="#main">"##));
}

#[test]
fn the_rail_marks_the_active_space_and_every_link_is_under_the_base() {
    let s = render(&fixtures::chrome(), Rail::Resources, &[], "/k");
    assert_eq!(s.matches("aria-current=\"page\"").count(), 1);
    assert!(
        s.contains(r#"<a class="ni on" href="/k/topics" aria-current="page">"#),
        "{s}"
    );
    for path in ["/k/", "/k/live", "/k/access", "/k/lab", "/k/settings"] {
        assert!(s.contains(&format!("href=\"{path}\"")), "{path}");
    }
    assert!(
        !s.contains("href=\"/live\"") && !s.contains("href=\"/static/"),
        "a link escaped the prefix"
    );
}

#[test]
fn the_top_bar_says_the_address_the_profile_and_what_is_on() {
    let mut chrome = fixtures::chrome();
    let s = render(&chrome, Rail::Home, &[], "/");
    assert!(s.contains(r#"aria-label="Copy address 127.0.0.1:9092""#));
    assert!(s.contains("Kafka 4 profile"));
    assert!(s.contains(r#"aria-label="Access: Open"#) && s.contains(r#"<i class="rec">"#));
    chrome.security = "SASL SCRAM + TLS".into();
    chrome.locked = true;
    let s = render(&chrome, Rail::Home, &[], "/");
    assert!(s.contains("SASL SCRAM + TLS") && !s.contains(r#"<i class="rec">"#));
}

#[test]
fn no_page_opens_a_stream_and_a_page_that_lists_regions_has_the_refresh_control_in_the_breadcrumb_row()
 {
    let none = render(&fixtures::chrome(), Rail::Home, &[], "/");
    assert!(
        !none.contains("data-stream") && !none.contains("data-refresh"),
        "a snapshot with nothing to refresh has no control"
    );
    let some = render(
        &fixtures::chrome(),
        Rail::Home,
        &["home-instance", "home-sentence"],
        "/k",
    );
    assert!(
        !some.contains("data-stream"),
        "no page subscribes by itself"
    );
    assert!(
        some.contains(r#"<div class="crumb-row"><nav class="crumbs""#)
            && some.contains(r#"data-regions="home-instance,home-sentence""#),
        "{some}"
    );
}

#[test]
fn the_guard_banner_is_on_the_page_only_when_the_console_is_unprotected() {
    let mut chrome = fixtures::chrome();
    assert!(!render(&chrome, Rail::Home, &[], "/").contains("Served beyond loopback, no login."));
    chrome.unprotected = true;
    let s = render(&chrome, Rail::Home, &[], "/");
    assert!(
        s.contains("<b>Served beyond loopback, no login.</b>"),
        "{s}"
    );
    assert!(s.contains(r#"role="alert""#));
}

#[test]
fn the_search_is_drawn_as_the_board_draws_it_and_not_offered() {
    let s = render(&fixtures::chrome(), Rail::Home, &[], "/");
    assert!(s.contains(
        r#"placeholder="Jump to a topic or group" aria-label="Jump to a topic or group" disabled"#
    ));
}
