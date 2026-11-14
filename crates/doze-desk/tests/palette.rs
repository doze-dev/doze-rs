//! ⌘K's model against the Go app's palette goldens (`internal/desk/view/testdata/shop-palette-*.json`, doze 23b73d6)
//! and every assertion of Go's `TestPaletteListsWhatIsAtHandThenAVerbThenNothing`: the shop fixture through a real
//! hub, keyed `shop-1` as Go's test keys it, beside a stopped `acme` that last started api and web.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines
)]

#[path = "common/mod.rs"]
mod common;

use common::{epoch, following};
use doze_desk::palette::{Ask, DoKind, Palette, RecentService, Scope, palette};
use doze_desk::{StackKey, StackView};
use serde_json::Value;
use std::path::Path;

fn golden(name: &str) -> Value {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("testdata/go/view/{name}.json"));
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

/// `v` with the empty left out, as Go's `omitempty` leaves them.
fn norm(v: &Value) -> Value {
    match v {
        Value::Object(m) => Value::Object(
            m.iter()
                .map(|(k, v)| (k.clone(), norm(v)))
                .filter(|(_, v)| {
                    !matches!(v, Value::Null)
                        && v != &Value::Array(vec![])
                        && v != &Value::String(String::new())
                })
                .collect(),
        ),
        Value::Array(a) => Value::Array(a.iter().map(norm).collect()),
        // Improvement, on purpose (as the overview's goldens allow): Go's view snapshot left the stack's idle time out,
        // so a dozing row said "quiet; sleeps soon"; the hub keeps it, so the row counts down.
        Value::String(s) => Value::String(
            regex::Regex::new(r"sleeps in [0-9:]+")
                .unwrap()
                .replace(s, "quiet; sleeps soon")
                .into_owned(),
        ),
        other => other.clone(),
    }
}

fn check(p: &Palette, name: &str) {
    let got = norm(&serde_json::to_value(p).unwrap());
    let want = norm(&golden(name));
    // Improvement, on purpose: ⌘K counts awake as the stack's strip does, dozing apart; Go counted shop's dozing cache
    // too, so ⌘K said 5 where the page said 4.
    let want = dozing_apart(want);
    assert_eq!(got, want, "\n got {got:#}\nwant {want:#}");
}

fn stacks(st: StackView) -> Vec<StackView> {
    let mut shop = st;
    shop.key = StackKey::new("shop-1");
    let mut acme = StackView::new(StackKey::new("acme"));
    acme.name = "acme".into();
    acme.dir = "/w/acme".into();
    acme.last_set = vec!["api".into(), "web".into()];
    vec![shop, acme]
}

fn ask<'a>(scope: Option<&'a Scope>, recent: &'a [RecentService], q: &'a str) -> Ask<'a> {
    Ask {
        current: "shop-1",
        inst: "",
        scope,
        recent,
        cmds: &[],
        query: q,
        now: epoch(),
        extra: &[],
    }
}

fn titles(p: &Palette) -> String {
    p.sections
        .iter()
        .map(|s| s.title.as_str())
        .collect::<Vec<_>>()
        .join(",")
}

#[test]
fn the_palette_is_gos() {
    let Some((_home, _d, hub, key)) = following("shop") else {
        return;
    };
    let all = stacks(hub.stack(&key).unwrap());
    let recent = [
        RecentService {
            key: "shop-1".into(),
            name: "db".into(),
        },
        RecentService {
            key: "shop-1".into(),
            name: "migrate".into(),
        },
    ];

    // Nothing typed: three commands for the stack on screen, Recent from prefs, then every stack to tab into.
    let p = palette(&all, &ask(None, &recent, ""));
    check(&p, "shop-palette-empty");
    assert_eq!(titles(&p), "Suggested,Recent,Stacks");
    let r = &p.sections[1].rows;
    assert!(r[0].note.starts_with("in shop · copy URL") && r[1].label == "Run migrate");
    let r = &p.sections[2].rows;
    assert_eq!(
        (r[1].label.as_str(), r[1].note.as_str(), r[2].doing.kind),
        ("acme", "stopped", DoKind::Folder)
    );

    // Inside a service: its commands only, the crumbs saying where.
    let db = Scope {
        key: "shop-1".into(),
        name: Some("db".into()),
    };
    let p = palette(&all, &ask(Some(&db), &[], ""));
    assert_eq!(
        (
            p.crumbs.join("›").as_str(),
            p.sections[0].rows[0].label.as_str()
        ),
        ("shop›db", "Sleep db")
    );
    let p = palette(&all, &ask(Some(&db), &[], "psql"));
    assert!(p.sections.len() == 1 && p.sections[0].rows[0].note.starts_with("psql "));

    // A name, then a command for it; a name, then words for its logs; an alias is the verb it means.
    assert_eq!(
        palette(&all, &ask(None, &[], "db restart")).sections[0].rows[0].label,
        "Restart db"
    );
    let p = palette(&all, &ask(None, &[], "api timeout"));
    let d = &p.sections[0].rows[0].doing;
    assert_eq!(
        (d.page.as_str(), d.text.as_str(), d.target.as_str()),
        ("logs", "timeout", "api")
    );
    assert_eq!(
        palette(&all, &ask(None, &[], "stop ca")).sections[0].title,
        "Sleep"
    );

    // A verb and a name: what it applies to, best match first, then Did you mean.
    let p = palette(&all, &ask(None, &[], "sleep ca"));
    check(&p, "shop-palette-sleep-ca");
    assert_eq!(titles(&p), "Sleep,Did you mean");

    // A name alone opens it; a stack's name too.
    let p = palette(&all, &ask(None, &[], "sho"));
    assert_eq!(
        (
            p.sections[0].rows[0].label.as_str(),
            p.sections[0].rows[0].doing.page.as_str()
        ),
        ("shop", "home")
    );

    // Nothing called that: the closest name, the verb carried into the box, never run on the guess.
    let n = palette(&all, &ask(None, &[], "restart cachee"))
        .nothing
        .unwrap();
    assert_eq!(
        (n.closest.as_str(), n.r#where.as_str(), n.fill.as_str()),
        ("cache", "shop or acme", "restart cache")
    );
    assert!(n.doing.is_none());
}

/// Go's "5 awake" for shop as the app says it: the stack's figure "4 awake", and Sleep everything's "4 awake · 1 dozing".
fn dozing_apart(v: Value) -> Value {
    match v {
        Value::Object(mut m) => {
            let sleep_all = m
                .get("label")
                .and_then(Value::as_str)
                .is_some_and(|l| l.starts_with("Sleep everything in"));
            if m.get("note").and_then(Value::as_str) == Some("5 awake") {
                let note = if sleep_all {
                    "4 awake · 1 dozing"
                } else {
                    "4 awake"
                };
                m.insert("note".into(), Value::String(note.into()));
            }
            Value::Object(m.into_iter().map(|(k, v)| (k, dozing_apart(v))).collect())
        }
        Value::Array(a) => Value::Array(a.into_iter().map(dozing_apart).collect()),
        other => other,
    }
}
