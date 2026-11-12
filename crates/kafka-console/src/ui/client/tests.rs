use super::*;

/// The script as shipped.
const SCRIPT: &str = include_str!("../../../assets/console.js");

#[test]
fn the_script_uses_every_name_this_file_defines() {
    for name in [
        EVENT_TICK,
        ATTR_FRAG,
        ATTR_AJAX,
        ATTR_TARGET,
        ATTR_STREAM,
        ATTR_JS_ONLY,
        ATTR_COPY,
        ATTR_COPY_FROM,
        ATTR_COPY_FORMS,
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
    // Only the client snippets follow "Copy as": the button that copies a card's own lines does not carry the attribute.
    let snippet = copy_snippet_button("btn", "t", html! { "Copy" }).into_string();
    assert!(
        snippet.contains(&format!(" {ATTR_COPY_FORMS}"))
            && snippet.contains(&format!(" {ATTR_COPY_FROM}=\"t\"")),
        "{snippet}"
    );
    assert!(!from.contains(ATTR_COPY_FORMS), "{from}");
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
    // The body boosts every link and form; the stream's address is on `main` (the shell's tests), where a swap renews it.
    let body = body(html! {}).into_string();
    assert!(
        body.starts_with(r##"<body hx-boost="true" hx-target="#main""##) && body.contains("preload=\"mouseover\""),
        "{body}"
    );
    let frag = fragment("home-instance", "/fragments/home-instance", html! { "x" }).into_string();
    assert!(
        frag.contains(&format!("class=\"{CLASS_FRAGMENT}\""))
            && frag.contains(&format!(" {ATTR_FRAG}=\"/fragments/home-instance\"")),
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
    assert!(f.starts_with(r#"<form method="post" action="/connect/advertised/check" data-ajax data-target="probe-home">"#), "{f}");
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

#[test]
fn the_script_follows_the_rules_the_spike_found_and_runs_under_the_strictest_policy() {
    // D4a (docs/port/phase9/client-spike.md): each trap is a line of the script, and a test that the line is still there.
    for (rule, needle) in [
        (
            "the script sends If-None-Match itself, a browser turns a 304 into a 200 for fetch",
            "'If-None-Match'",
        ),
        ("and skips the swap on a 304", "r.status === 304"),
        (
            "the newest request wins per fragment",
            "new AbortController()",
        ),
        (
            "the stream is closed when the page is hidden",
            "visibilitychange",
        ),
        ("and when it goes to the back/forward cache", "'pagehide'"),
        ("and reopened when it comes back", "'pageshow'"),
        (
            "the focused input keeps its value",
            "focused && name === 'value'",
        ),
        (
            "the reconnect waits longer and longer",
            "Math.min(wait * 2, 15000)",
        ),
    ] {
        assert!(
            SCRIPT.contains(needle),
            "{rule}: {needle:?} is not in console.js"
        );
    }
    // Never gives up: no retry counter, no limit on reconnecting.
    assert!(
        !SCRIPT.contains("retries") && !SCRIPT.contains("giveUp") && !SCRIPT.contains("attempts")
    );
    // Runs under `default-src 'self'` with no unsafe-eval: nothing evaluates text, nothing is injected inline.
    for banned in [
        "eval(",
        "new Function",
        "setTimeout(\"",
        "document.write",
        "importScripts",
        "javascript:",
    ] {
        assert!(!SCRIPT.contains(banned), "console.js uses {banned:?}");
    }
    assert!(
        SCRIPT.to_ascii_lowercase().contains(HEADER_FRAGMENT),
        "the form header is {HEADER_FRAGMENT}"
    );
    // (Slice 9e's Live adds about 90 lines: pause with a held-back count, marked new rows, j and k, select filters; the DOM tests
    // in `tools/dom/live.test.ts` run them. The spike's flip condition of about 200 lines of client logic is crossed: see the 9e report.)
    // (Over 130 since the audit asked for the clipboard fallback and the banner's way back; the behaviour is tested in
    // `tools/dom`, which runs this very file, so these are no longer the only guard.)
    assert!(
        SCRIPT.lines().count() < 520,
        "the script is small enough to read in one sitting"
    );
}

#[test]
fn a_dialog_link_is_a_plain_link_a_script_can_open_over_the_page() {
    let a = dialog_link("btn pri", "/topics/orders/produce", html! { "Produce" }).into_string();
    assert_eq!(
        a,
        format!(r#"<a class="btn pri" href="/topics/orders/produce" {ATTR_DIALOG}>Produce</a>"#)
    );
    assert!(
        super::super::dialog::cancel("/t")
            .into_string()
            .contains(ATTR_DIALOG_CLOSE)
    );
}

#[test]
fn the_script_opens_dialogs_by_the_names_this_file_defines() {
    for name in [
        ATTR_DIALOG,
        ATTR_DIALOG_CLOSE,
        ATTR_OVERLAY,
        ATTR_PALETTE,
        ATTR_TRANSIENT,
        ATTR_JUMP_INPUT,
        ATTR_JUMP,
        HEADER_OVERLAY,
        CLASS_OVERLAY_IN,
        CLASS_OVERLAY_OUT,
    ] {
        assert!(SCRIPT.contains(name), "console.js never mentions {name:?}");
    }
    // An overlay is ours (the markup of src/ui/overlay.rs): the address is pushed, Back closes it, the page behind is inert.
    for needle in ["history.pushState", "popstate", "'inert'", "Escape"] {
        assert!(SCRIPT.contains(needle), "{needle:?}");
    }
    // The answer is a fragment (the dialog's body), asked for in the header the server looks for.
    assert!(SCRIPT.contains("'X-Fragment'"));
    // A dialog that cannot be fetched falls back to following the link.
    assert!(SCRIPT.contains("location.href"));
}
