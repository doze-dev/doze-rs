//! Every state on every screen. The `states` fixture (captured from Go's `fixture.States`: one instance in each state
//! doze has) is read by each of the app's surfaces as doze-desk builds them, and each surface's state word must be the
//! words' own (`doze_display::look`): the overview's row and tile, the pane beside the List, the sidebar, the instance
//! page's head, the strip's counts and ⌘K's stack figure. Then the app's own source is read for state words written by
//! hand, which is how two screens come to disagree.
//!
//! `DOZE_STATES_MATRIX=<file>` also writes the matrix (instance × surface: the word each shows) as JSON, for the
//! contact sheet.
#![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]

use doze_control::fixture::Script;
use doze_control::{Response, Timestamp};
use doze_desk::overview::{home, pane};
use doze_desk::page::instance::instance_page;
use doze_desk::page::side::side;
use doze_desk::page::time::Zone;
use doze_desk::{Conn, StackKey, StackView};
use doze_display::{self as words, Look};
use serde_json::{Value, json};
use std::path::Path;
use std::sync::Arc;
use std::time::SystemTime;

fn epoch() -> SystemTime {
    serde_json::from_str::<Timestamp>("\"2026-10-11T14:04:40Z\"")
        .unwrap()
        .to_system_time()
        .unwrap()
}

/// The `states` fixture's stack, as the hub would hold it once its snapshot arrived.
fn states() -> StackView {
    let s = Script::load(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../doze-control/testdata/go/scripts/states.ndjson"),
    )
    .unwrap();
    let r: Response = s.status;
    let mut st = StackView::new(StackKey::new("states-1"));
    st.name = r.stack.clone();
    st.dir = r.dir.clone();
    st.running = true;
    st.link.conn = Conn::Live;
    st.idle_timeout = std::time::Duration::from_secs(300);
    let mut ins = r.instances.clone();
    ins.sort_by(|a, b| a.name.cmp(&b.name));
    st.instances = Arc::new(ins);
    st
}

/// The states the fixture must hold, one each: if one goes missing from the fixture, the matrix has a hole.
const LOOKS: [Look; 11] = Look::ALL;

#[test]
fn the_fixture_has_every_look() {
    let st = states();
    let snap = st.words();
    for l in LOOKS {
        assert!(
            snap.instances.iter().any(|v| words::look(v) == l),
            "no instance in the states fixture looks {}",
            l.word()
        );
    }
    // And the rest of the vocabulary: pinned, held by a dependant, waiting on, woken by, not ready, gave up.
    let because: Vec<String> = snap
        .instances
        .iter()
        .map(|v| words::because(v, &snap, epoch()))
        .collect();
    for want in [
        "pinned awake",
        "api needs it",
        "waiting on events",
        "woken by worker",
    ] {
        assert!(
            because.iter().any(|b| b.contains(want))
                || snap
                    .instances
                    .iter()
                    .any(|v| words::what(v, &snap, epoch()).contains(want)),
            "nothing in the fixture says {want:?}"
        );
    }
    assert!(
        snap.instances
            .iter()
            .any(|v| v.last_error.contains("not become ready"))
    );
    assert!(
        snap.instances
            .iter()
            .any(|v| v.last_error.contains("gave up"))
    );
}

#[test]
fn every_surface_says_the_words_state() {
    let st = states();
    let snap = st.words();
    let now = epoch();
    let h = home(&st, now);
    let rows: Vec<_> = h.groups.iter().flat_map(|g| &g.rows).collect();
    let sides = side(&snap);
    let mut matrix = Vec::new();
    for v in &snap.instances {
        let want = words::look(v);
        let row = rows.iter().find(|r| r.name == v.name).unwrap();
        assert_eq!(row.look.word, want.word(), "{}: the overview's row", v.name);
        assert_eq!(row.look.glyph, want.glyph(), "{}: the row's glyph", v.name);
        assert_eq!(
            row.look.tone,
            want.tone().as_str(),
            "{}: the row's tone",
            v.name
        );
        assert_eq!(row.on, words::switch_on(v), "{}: the row's switch", v.name);
        let p = pane(&st, &v.name, now).unwrap();
        assert_eq!(p.look.word, want.word(), "{}: the List's pane", v.name);
        let s = sides.iter().find(|s| s.name == v.name).unwrap();
        assert_eq!(s.look, want, "{}: the sidebar", v.name);
        let page = instance_page(&st, &v.name, None, now, Zone::UTC);
        assert_eq!(
            page.look,
            Some(want),
            "{}: the instance page's head",
            v.name
        );
        matrix.push(json!({
            "name": v.name,
            "look": want.word(),
            "glyph": want.glyph(),
            "tone": want.tone().as_str(),
            "switch": if v.kind == words::Kind::Task { "Run" } else if v.paused { "⊘" } else if words::switch_on(v) { "on" } else { "off" },
            "row": format!("{} · {}", row.look.word, row.because),
            "side": s.note,
            "trouble": words::troubled(v),
        }));
    }
    // The strip counts each state once, in the words' words; every non-task instance is in exactly one count.
    let s8 = &h.vitals.state;
    let tasks = snap
        .instances
        .iter()
        .filter(|v| v.kind == words::Kind::Task)
        .count();
    assert_eq!(
        s8.awake + s8.dozing + s8.waking + s8.asleep + s8.off + s8.paused + s8.crashing + s8.failed,
        snap.instances.len() - tasks,
        "the strip's counts cover every service once"
    );
    if let Some(out) = std::env::var_os("DOZE_STATES_MATRIX") {
        std::fs::write(
            out,
            serde_json::to_string_pretty(&Value::Array(matrix)).unwrap(),
        )
        .unwrap();
    }
}

/// A state word typed into a surface instead of taken from the words is how two screens disagree (Go's ⌘K said "6
/// awake" where the page said 5). Every string literal in the app and in doze-desk that is a state word, or a count of
/// one ("{} awake"), must come from `doze_display::Look` (or the stopped look) instead. The verbs (`off`, a result's
/// `failed` on the wire) are allowed where they are verbs.
#[test]
fn no_screen_writes_a_state_word_by_hand() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let states = [
        "awake", "dozing", "waking", "asleep", "crashing", "paused", "not run", "stopped",
    ];
    // file (from the workspace root), literal: why it may stay.
    let allowed: &[(&str, &str)] = &[
        ("crates/doze-desk/src/overview/mod.rs", "stopped"), // the stopped look itself: its one definition
        ("apps/doze-app/src/overlays/help.rs", "pinned"),    // a concept, not a look
        ("apps/doze-app/src/logs/toolbar.rs", "paused"), // the logs' follow switch, not an instance
        ("apps/doze-app/src/fixture.rs", "crashing"),    // a synthetic stack's wire state
        ("apps/doze-app/src/instance/asleep.rs", "asleep"), // an element id
        ("apps/doze-app/src/instance/overview.rs", "awake"), // the card a module's block goes after (`after = "awake"`)
    ];
    let mut found = Vec::new();
    for dir in ["apps/doze-app/src", "crates/doze-desk/src"] {
        for file in rust_files(&root.join(dir)) {
            let rel = file
                .strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if rel.contains("/map/") || rel.ends_with("tests.rs") || rel.contains("/tests/") {
                continue;
            }
            let text = std::fs::read_to_string(&file).unwrap();
            for (n, line) in text.lines().enumerate() {
                let code = line.split("//").next().unwrap_or("");
                for lit in literals(code) {
                    let bare = lit.trim();
                    let counted = bare
                        .strip_prefix('{')
                        .and_then(|r| r.split_once("} "))
                        .map(|(_, w)| w.trim());
                    let hit = states.iter().find(|w| {
                        bare == **w || counted == Some(**w) || bare.ends_with(&format!("{{}} {w}"))
                    });
                    if let Some(w) = hit
                        && !allowed.iter().any(|(f, a)| rel == *f && a == w)
                    {
                        found.push(format!("{rel}:{}: \"{lit}\"", n + 1));
                    }
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "state words written by hand (take them from doze_display::Look):\n{}",
        found.join("\n")
    );
}

fn rust_files(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(rust_files(&p));
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
    out
}

/// The plain string literals on a line (no raw strings; escapes kept as written).
fn literals(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = line;
    while let Some(start) = rest.find('"') {
        let after = &rest[start + 1..];
        let mut end = None;
        let mut esc = false;
        for (i, c) in after.char_indices() {
            match c {
                '\\' if !esc => esc = true,
                '"' if !esc => {
                    end = Some(i);
                    break;
                }
                _ => esc = false,
            }
        }
        let Some(end) = end else { break };
        out.push(&after[..end]);
        rest = &after[end + 1..];
    }
    out
}
