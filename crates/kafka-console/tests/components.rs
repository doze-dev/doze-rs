//! Golden files for the shell and the main components: the HTML each one writes, kept in `tests/golden/`.
//! `DOZE_UPDATE_DESIGN=1 cargo test -p kafka-console --test components` rewrites them; a diff in review is the point.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::path::Path;

use kafka_console::assets::AssetUrls;
use kafka_console::base::Base;
use kafka_console::model::Tok;
use kafka_console::ui::button::{Size, State, Variant, link, pill_mono, state};
use kafka_console::ui::client;
use kafka_console::ui::code::{block, pre};
use kafka_console::ui::filter::{input, seg};
use kafka_console::ui::form::{field, teach};
use kafka_console::ui::glyph::{Glyph, glyph};
use kafka_console::ui::layout::{card_extra, hr, kv, page_header, title_row, value_with_sub};
use kafka_console::ui::notice::{Tone, banner_bad, callout};
use kafka_console::ui::shell::{Chrome, Crumb, Page, Rail, document};
use kafka_console::ui::table::{Column, table};
use maud::{Markup, html};

/// Compares `markup` with `tests/golden/<name>.html`, one element per line so a diff reads.
fn golden(name: &str, markup: &Markup) {
    let text = markup.0.replace("><", ">\n<") + "\n";
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(format!("{name}.html"));
    if std::env::var_os("DOZE_UPDATE_DESIGN").is_some() {
        std::fs::write(&path, &text).unwrap();
    }
    let want = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("no golden file {name}.html: run with DOZE_UPDATE_DESIGN=1"));
    assert!(
        want == text,
        "{name}.html differs from what the component writes; run with DOZE_UPDATE_DESIGN=1 and read the diff"
    );
}

#[test]
fn the_shell() {
    let base = Base::new("/k");
    let urls = AssetUrls {
        css: "/k/static/console.css".into(),
        scripts: vec!["/k/static/console.js".into()],
        favicon: "/k/static/favicon.svg".into(),
    };
    let chrome = Chrome {
        address: "127.0.0.1:9092".into(),
        profile: "Kafka 4 profile".into(),
        security: "SASL SCRAM + TLS".into(),
        locked: true,
        unprotected: true,
    };
    let page = Page {
        base: &base,
        assets: &urls,
        chrome: &chrome,
        active: Rail::Resources,
        crumbs: vec![
            Crumb {
                label: "Home".into(),
                href: Some("/k/".into()),
            },
            Crumb {
                label: "Connect an app".into(),
                href: None,
            },
        ],
        title: "Connect an app",
        regions: &["home-instance", "home-sentence"],
    };
    golden("shell", &document(&page, html! { h1 { "Connect an app" } }));
}

#[test]
fn buttons_states_and_pills() {
    golden(
        "buttons",
        &html! {
            (link("/a", Variant::Plain, Size::Normal, None, "Plain"))
            (link("/a", Variant::Primary, Size::Normal, Some(Glyph::Code), "Primary"))
            (link("/a", Variant::Danger, Size::Small, Some(Glyph::X), "Danger"))
            (state(State::Ok, "Reachable"))
            (state(State::Warn, "Waiting"))
            (state(State::Bad, "Not reachable"))
            (state(State::Refused, "Refused"))
            (state(State::Info, "Info"))
            (state(State::Idle, "Listening"))
            (pill_mono("c · py"))
        },
    );
}

#[test]
fn notices() {
    golden(
        "notices",
        &html! {
            (callout(Tone::Plain, Glyph::Check, html! { b { "Nothing needs you." } " All clear." }))
            (callout(Tone::Warn, Glyph::Warn, html! { b { "1 thing needs you." } }))
            (callout(Tone::Bad, Glyph::X, html! { "Bad" }))
            (callout(Tone::Refused, Glyph::Info, html! { "Refused" }))
            (banner_bad(Some("stream-banner"), true, html! { b { "Live updates are paused." } }))
        },
    );
}

#[test]
fn layout() {
    golden(
        "layout",
        &html! {
            (page_header("Home", html! { p class="sub" { "Running " b { "3 d 4 h" } } }, link("/connect", Variant::Primary, Size::Normal, Some(Glyph::Code), "Connect an app")))
            (card_extra("endpoint", html! {
                (title_row("Address", html! { a class="lnk" href="/connect" { "Snippets" } }))
                (hr())
                (kv(vec![
                    ("Started", html! { span class="mono" { "Nov 6 18:02 UTC" } }),
                    ("Open partitions", value_with_sub(html! { span class="mono" { "21 of 64" } }, "Opened on first use")),
                ]))
                (glyph(Glyph::Copy, 16, "d-data"))
            }))
        },
    );
}

#[test]
fn table_form_filter_and_code() {
    let cols = [
        Column {
            name: "Topic",
            right: false,
        },
        Column {
            name: "Records",
            right: true,
        },
    ];
    golden(
        "components",
        &html! {
            (table(&cols, vec![vec![html! { a href="/topics/orders" { "orders" } }, html! { "48,213" }]]))
            (field("topic-name", "Topic name", Some("Up to 249 bytes"), html! { input id="topic-name" name="name" class="input"; }))
            (teach("Partitions are fixed up front.", "You can add more later, never take them away."))
            (input("q", "Filter topics", ""))
            (seg(html! {
                (client::copy_as_button("code", true, "Code"))
                (client::copy_as_button("properties", false, "Properties"))
            }))
            (block(&[vec![(Tok::Keyword, "cl".into()), (Tok::Plain, ", err := kgo.NewClient(".into())], vec![(Tok::Str, "\"127.0.0.1:9092\"".into())]], Some("snippet-0"), None))
            (pre("kcat -b 127.0.0.1:9092 -L", None, Some("margin-top:8px")))
            (client::copy_button("btn sm", "127.0.0.1:9092", html! { "Copy" }))
            (client::fragment("home-instance", "/fragments/home-instance", html! { "x" }))
            (client::form("/connect/advertised/check", "probe-home", &[("back", "home")], html! { button type="submit" { "Check again" } }))
        },
    );
}
