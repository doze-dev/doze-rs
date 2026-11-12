use super::*;

/// The script as shipped.
const SCRIPT: &str = include_str!("../../../assets/console.js");

#[test]
fn the_script_uses_every_name_this_file_defines() {
    for name in [
        EVENT_TICK,
        EVENT_REFRESH,
        ATTR_STREAM,
        ATTR_JS_ONLY,
        ATTR_COPY,
        ATTR_COPY_FROM,
        ATTR_COPY_AS,
        ATTR_ADDRESS,
        ID_BANNER,
    ] {
        assert!(SCRIPT.contains(name), "console.js never mentions {name:?}");
    }
}

#[test]
fn the_markup_uses_the_attribute_names_the_constants_say() {
    let copy = copy_button("btn", "x", html! { "Copy" }).into_string();
    assert!(
        copy.contains(&format!(" {ATTR_COPY}=\"x\""))
            && copy.contains(ATTR_JS_ONLY)
            && copy.contains(" hidden"),
        "{copy}"
    );
    let from = copy_from_button("btn", "t", html! { "Copy" }).into_string();
    assert!(from.contains(&format!(" {ATTR_COPY_FROM}=\"t\"")), "{from}");
    let seg = copy_as_button("code", true, "Code").into_string();
    assert!(
        seg.contains(&format!(" {ATTR_COPY_AS}=\"code\"")) && seg.contains("class=\"on\""),
        "{seg}"
    );
    let holder = address_holder("1.2.3.4:5", html! {}).into_string();
    assert!(
        holder.contains(&format!(" {ATTR_ADDRESS}=\"1.2.3.4:5\"")),
        "{holder}"
    );
    let body = body(Some("/events?watch=a"), html! {}).into_string();
    assert!(
        body.contains(&format!(" {ATTR_STREAM}=\"/events?watch=a\"")),
        "{body}"
    );
    let frag = fragment("home-instance", "/fragments/home-instance", html! { "x" }).into_string();
    assert!(
        frag.contains(&format!("class=\"{CLASS_FRAGMENT}\""))
            && frag.contains(&format!("hx-trigger=\"{EVENT_REFRESH}\"")),
        "{frag}"
    );
}

#[test]
fn a_form_posts_with_no_script_and_swaps_its_target_with_one() {
    let f = form(
        "/connect/advertised/check",
        "probe-home",
        &[("back", "home")],
        html! { button { "Go" } },
    )
    .into_string();
    assert!(f.starts_with(r##"<form method="post" action="/connect/advertised/check" hx-post="/connect/advertised/check" hx-target="#probe-home" hx-swap="outerHTML">"##), "{f}");
    assert!(
        f.contains(r#"<input type="hidden" name="back" value="home">"#),
        "{f}"
    );
}

#[test]
fn the_stream_url_lists_the_watched_fragments() {
    assert_eq!(stream_url("/k/events", &["a", "b"]), "/k/events?watch=a,b");
    assert_eq!(stream_url("/events", &[]), "/events?watch=");
}

#[test]
fn nothing_the_script_needs_is_shown_without_it() {
    // A control that only a script can serve is hidden in the markup itself, and the style keeps it hidden.
    assert!(CSS.contains("[hidden]"));
    assert!(
        copy_button("btn", "x", html! {})
            .into_string()
            .contains(" hidden")
    );
}
