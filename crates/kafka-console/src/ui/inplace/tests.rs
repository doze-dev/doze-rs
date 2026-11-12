use super::*;

#[test]
fn a_link_is_a_plain_link_that_asks_for_one_region() {
    let m = link(
        "x-view",
        "fchip on",
        "/x?a=1",
        Some("Remove a"),
        html! { "A" },
    )
    .into_string();
    assert!(
        m.starts_with(r##"<a class="fchip on" href="/x?a=1" aria-label="Remove a" hx-get="/x?a=1" hx-target="#x-view" hx-select="#x-view" hx-swap="morph:outerHTML show:none" hx-push-url="true">"##),
        "{m}"
    );
}

#[test]
fn a_toolbar_form_is_a_plain_get_that_submits_in_place() {
    let m = form(
        "x-view",
        Some("bar"),
        "toolbar",
        None,
        "/x",
        html! { button { "Go" } },
    )
    .into_string();
    assert!(
        m.contains(r#"method="get" action="/x""#)
            && m.contains(r##"hx-target="#x-view""##)
            && m.contains(r#"id="bar""#),
        "{m}"
    );
    assert!(
        m.contains(r#"hx-trigger="submit""#) && m.contains(r#"hx-push-url="true""#),
        "{m}"
    );
}

#[test]
fn a_search_box_waits_for_the_last_key_replaces_the_history_and_drops_the_older_request() {
    let m = search_box("x-view", "/x", "q", "Filter", "Filter x", "a\"b").into_string();
    assert!(
        m.contains(r#"hx-trigger="input changed delay:250ms, search""#),
        "{m}"
    );
    assert!(
        m.contains(r#"hx-replace-url="true""#) && m.contains(r#"hx-push-url="false""#),
        "{m}"
    );
    assert!(
        m.contains(r#"hx-sync="this:replace""#) && m.contains(r#"hx-include="closest form""#),
        "{m}"
    );
    assert!(
        m.contains(r#"hx-headers="{&quot;x-history&quot;:&quot;replace&quot;}""#),
        "{m}"
    );
    assert!(
        m.contains(r#"value="a&quot;b""#),
        "a hostile value is escaped: {m}"
    );
}

#[test]
fn a_region_is_a_wrapper_with_the_id() {
    assert_eq!(
        region("x-view", html! { "c" }).into_string(),
        r#"<div class="frag" id="x-view">c</div>"#
    );
}
