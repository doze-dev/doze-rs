//! The pages inside a stack (`doze_desk::page`) against the Go app's view goldens: each page is written out the way
//! Go's JSON writes it (its field names, `omitempty`) and compared with the golden, field for field. What the Rust
//! page leaves out by design is said where it is skipped.

use super::check::{g, golden, s};
use super::common::{epoch, following, until};
use doze_control::Topic;
use doze_desk::page::instance::{InstancePage, instance_page};
use doze_desk::page::logindex::{LogIndex, LogQuery};
use doze_desk::page::parts::Kv;
use doze_desk::page::time::Zone;
use doze_desk::page::{Ink, logs};
use doze_desk::{Hub, StackKey, StackView};
use serde_json::{Map, Value, json};

/// The zone the goldens were written in.
const IST: Zone = Zone(19_800);

pub(super) fn ink(i: Ink) -> &'static str {
    match i {
        Ink::Good => "good",
        Ink::Warn => "warn",
        Ink::Cool => "cool",
        Ink::Bad => "bad",
        Ink::Dim => "dim",
        Ink::Text => "text",
        Ink::Bright => "bright",
        Ink::Accent => "accent",
    }
}

/// Go's `omitempty`: drops the keys whose value is empty, false or zero.
pub(super) fn put(m: &mut Map<String, Value>, k: &str, v: Value) {
    let empty = match &v {
        Value::Null => true,
        Value::Bool(b) => !b,
        Value::String(s) => s.is_empty(),
        Value::Array(a) => a.is_empty(),
        Value::Number(n) => n.as_f64() == Some(0.0),
        Value::Object(_) => false,
    };
    if !empty {
        m.insert(k.to_owned(), v);
    }
}

fn kv(r: &Kv) -> Value {
    let mut m = Map::new();
    m.insert("k".into(), json!(r.k));
    m.insert("v".into(), json!(r.v));
    put(&mut m, "copy", json!(r.copy));
    m.insert("mono".into(), json!(r.mono));
    put(&mut m, "dim", json!(r.dim));
    put(&mut m, "note", json!(r.note));
    Value::Object(m)
}

fn shop() -> Option<(
    super::common::Home,
    doze_control::fixture::FixtureDaemon,
    Hub,
    StackKey,
    StackView,
)> {
    let (home, d, hub, key) = following("shop")?;
    let w = (hub.watch(&key, Topic::Stats), hub.watch(&key, Topic::Logs));
    let st = until(&hub, &key, "the meters, logs and journal", |s| {
        super::common::played(s, "shop") && s.logs_added >= 23 && s.events.len() == 11
    });
    drop(w);
    Some((home, d, hub, key, st))
}

fn page_of(hub: &Hub, key: &StackKey, st: &StackView, name: &str) -> InstancePage {
    hub.with_logs(key, |r| instance_page(st, name, Some(r), epoch(), IST))
        .unwrap()
}

/// The head, the cards and the sidebar of a page, against its golden.
fn check_instance(p: &InstancePage, want: &Value) {
    let name = &p.name;
    let look = p.look.unwrap();
    assert_eq!(look.word(), s(g(want, "/look/word")), "{name}");
    assert_eq!(look.glyph(), s(g(want, "/look/glyph")), "{name}");
    assert_eq!(p.on, g(want, "/on").as_bool().unwrap(), "{name}");
    for (got, path) in [
        (&p.path, "/path"),
        (&p.because, "/because"),
        (&p.up, "/up"),
        (&p.run, "/run"),
        (&p.sleeps, "/sleeps"),
    ] {
        assert_eq!(got, s(g(want, path)), "{name} {path}");
    }
    assert_eq!(p.pid, g(want, "/pid").as_i64().unwrap_or(0), "{name}");
    // The Go view's rows; the Rust page adds the process's IP and port as Go's service did on top of them.
    let rows: Vec<Value> = p
        .connect
        .iter()
        .filter(|r| r.k != "IP and port")
        .map(kv)
        .collect();
    assert_eq!(
        Value::Array(rows),
        g(want, "/connect")
            .as_array()
            .cloned()
            .map_or(json!([]), Value::Array),
        "{name}"
    );
    let holders: Vec<Value> = p
        .holders
        .iter()
        .map(|h| json!({"text": h.text, "explain": h.explain}))
        .collect();
    assert_eq!(
        Value::Array(holders),
        g(want, "/holders")
            .as_array()
            .cloned()
            .map_or(json!([]), Value::Array)
    );
    let tabs: Vec<Value> = p
        .tabs
        .iter()
        .map(|t| json!({"id": t.id, "title": t.title}))
        .collect();
    assert_eq!(Value::Array(tabs), *g(want, "/tabs"), "{name}");
    let needs: Vec<Value> = p.needs.iter().map(|n| json!({
        "name": n.name, "known": n.known, "glyph": n.glyph, "task": n.task,
        "look": n.look.map_or(json!(null), |l| json!({"word": l.word(), "glyph": l.glyph(), "tone": l.tone().as_str()})),
    })).collect();
    assert_eq!(
        Value::Array(needs),
        g(want, "/needs")
            .as_array()
            .cloned()
            .map_or(json!([]), Value::Array),
        "{name}"
    );
    let side: Vec<Value> = p.side.iter().map(|x| json!({
        "name": x.name, "look": {"word": x.look.word(), "glyph": x.look.glyph(), "tone": x.look.tone().as_str()},
        "note": x.note, "trouble": x.trouble,
    })).collect();
    let want_side: Vec<Value> = g(want, "/side")
        .as_array()
        .unwrap()
        .iter()
        .map(|x| {
            let mut x = x.clone();
            x.as_object_mut().unwrap().remove("kind");
            x
        })
        .collect();
    assert_eq!(side, want_side, "{name}'s sidebar");
    let recent: Vec<Value> = p
        .recent
        .iter()
        .map(|r| json!({"at": r.at, "kind": r.kind, "text": r.text, "tone": ink(r.ink)}))
        .collect();
    assert_eq!(
        Value::Array(recent),
        // Go left a detail-less event's colon in ("failed" · "in 400ms: "); the app drops it, on purpose.
        g(want, "/recent").as_array().map_or(json!([]), |rs| {
            Value::Array(
                rs.iter()
                    .map(|r| {
                        let mut r = r.clone();
                        for k in ["kind", "text"] {
                            let v = s(g(&r, &format!("/{k}")))
                                .trim()
                                .trim_end_matches(':')
                                .to_owned();
                            r[k] = json!(v);
                        }
                        r
                    })
                    .collect(),
            )
        }),
        "{name}"
    );
    match (&p.vitals, g(want, "/vitals")) {
        (None, Value::Null) => {}
        (Some(v), w) => {
            for (c, k) in [(&v.ram, "ram"), (&v.cpu, "cpu"), (&v.conns, "conns")] {
                let got = json!({"lo": c.lo, "hi": c.hi, "now": c.now, "sub": c.sub, "peak": c.peak, "mean": c.mean, "span": c.span});
                let mut w = g(w, &format!("/{k}")).clone();
                w.as_object_mut().unwrap().remove("points");
                for f in ["lo", "hi"] {
                    let x = w[f].as_f64().unwrap();
                    w[f] = json!(x);
                }
                // Deliberately not Go's: "<0.01 cores" is followed by "0% of one core", not "cores · 0% of one"
                // again, and one connection is "1 connection".
                let sub = w["sub"].as_str().unwrap().to_owned();
                if let Some(pct) = sub.strip_prefix("cores · ") {
                    w["sub"] = json!(format!("{pct} core"));
                } else if k == "conns" && c.now == "1" {
                    w["sub"] = json!("connection");
                }
                assert_eq!(got, w, "{name}'s {k}");
            }
        }
        (None, w) => panic!("{name} has no vitals; Go has {w}"),
    }
}

#[test]
fn shop_instances() {
    let Some((_home, _d, hub, key, st)) = shop() else {
        return;
    };
    for name in ["api", "db", "cloud", "migrate"] {
        let file = format!("shop-{name}");
        let want = golden(&file);
        check_instance(&page_of(&hub, &key, &st, name), &want);
    }
    let migrate = page_of(&hub, &key, &st, "migrate");
    let runs: Vec<Value> = migrate.runs.iter().map(|r| json!({
        "seq": r.seq, "when": r.when, "ago": r.ago, "why": r.why, "took": r.took, "secs": r.secs,
        "ok": r.ok, "done": r.done, "exit": r.exit,
    })).collect();
    let want_runs: Vec<Value> = g(&golden("shop-migrate"), "/runs")
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            let mut r = r.clone();
            r.as_object_mut().unwrap().remove("at");
            r
        })
        .collect();
    assert_eq!(runs, want_runs);
}

#[test]
fn a_crash() {
    let Some((_home, _d, hub, key)) = following("crash") else {
        return;
    };
    let _w = (hub.watch(&key, Topic::Stats), hub.watch(&key, Topic::Logs));
    let st = until(&hub, &key, "the script played to its end", |s| {
        super::common::played(s, "crash")
    });
    let want = golden("crash-api");
    let p = page_of(&hub, &key, &st, "api");
    check_instance(&p, &want);
    let c = p.crash.unwrap();
    let got = json!({"error": c.error, "where": c.r#where, "tries": c.tries, "lastTry": c.last_try,
        "fraction": c.fraction, "nextIn": c.next_in, "gaveUp": c.gave_up});
    assert_eq!(got, *g(&want, "/crash"));
    assert_eq!(c.port, None, "a panic is not a port clash");
}

fn line(l: &logs::Line) -> Value {
    let mut m = Map::new();
    put(&mut m, "instance", json!(l.instance));
    m.insert("time".into(), json!(l.time));
    m.insert("rel".into(), json!(l.rel));
    m.insert("line".into(), json!(l.text));
    put(&mut m, "level", json!(l.level));
    m.insert("rank".into(), json!(l.rank));
    put(&mut m, "run", json!(l.run));
    put(&mut m, "doze", json!(l.doze));
    Value::Object(m)
}

fn check_logs(
    hub: &Hub,
    key: &StackKey,
    st: &StackView,
    q: LogQuery,
    file: &str,
    from: usize,
    n: usize,
) {
    let want = golden(file);
    let mut idx = LogIndex::new(q);
    let lines = hub
        .with_logs(key, |r| {
            idx.update(r, &st.events);
            idx.rows(r, from..from + n, epoch(), IST)
        })
        .unwrap();
    assert_eq!(
        idx.len() as u64,
        g(&want, "/total").as_u64().unwrap(),
        "{file}"
    );
    assert_eq!(
        Value::Array(lines.iter().map(line).collect()),
        *g(&want, "/lines"),
        "{file}"
    );
    assert_eq!(json!(idx.runs()), *g(&want, "/runs"), "{file}");
    assert_eq!(json!(idx.counts()), *g(&want, "/counts"), "{file}");
    assert_eq!(json!(idx.matches()), *g(&want, "/matches"), "{file}");
    assert_eq!(json!(idx.instances()), *g(&want, "/instances"), "{file}");
}

#[test]
fn shop_logs_pages() {
    let Some((_home, _d, hub, key, st)) = shop() else {
        return;
    };
    let api = LogQuery {
        instance: "api".into(),
        level: 1,
        ..LogQuery::default()
    };
    check_logs(&hub, &key, &st, api, "shop-api-logs", 0, 5);
    let warn = LogQuery {
        level: 2,
        ..LogQuery::default()
    };
    check_logs(&hub, &key, &st, warn, "shop-all-logs-warn", 0, 1);
}
