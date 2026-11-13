//! The pages inside a stack (`doze_desk::page`) against the Go app's view goldens: each page is written out the way
//! Go's JSON writes it (its field names, `omitempty`) and compared with the golden, field for field. What the Rust
//! page leaves out by design is said where it is skipped.

use super::check::{g, golden, s};
use super::common::{epoch, following, until};
use doze_control::Topic;
use doze_desk::page::console::{self, ConsolePage, Node};
use doze_desk::page::instance::{InstancePage, instance_page};
use doze_desk::page::logindex::{LogIndex, LogQuery};
use doze_desk::page::parts::Kv;
use doze_desk::page::time::Zone;
use doze_desk::page::{Ink, logs};
use doze_desk::{Hub, StackKey, StackView};
use serde_json::{Map, Value, json};

/// The zone the goldens were written in.
const IST: Zone = Zone(19_800);

fn ink(i: Ink) -> &'static str {
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
fn put(m: &mut Map<String, Value>, k: &str, v: Value) {
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

fn node(n: &Node) -> Value {
    let mut m = Map::new();
    m.insert("kind".into(), json!(n.kind.as_str()));
    m.insert("id".into(), json!(n.id));
    put(&mut m, "title", json!(n.title));
    m.insert("weight".into(), json!(n.weight));
    put(&mut m, "flexible", json!(n.flexible));
    put(
        &mut m,
        "children",
        Value::Array(n.children.iter().map(node).collect()),
    );
    let cols: Vec<Value> = n
        .cols
        .iter()
        .map(|c| {
            let mut m = Map::new();
            m.insert("key".into(), json!(c.key));
            m.insert("title".into(), json!(c.title));
            put(&mut m, "align", json!(if c.right { "right" } else { "" }));
            Value::Object(m)
        })
        .collect();
    put(&mut m, "cols", Value::Array(cols));
    let rows: Vec<Value> = n
        .rows
        .iter()
        .map(|r| {
            let cells: Vec<Value> = r
                .cells
                .iter()
                .map(|c| json!({"text": c.text, "tone": ink(c.ink)}))
                .collect();
            let mut m = Map::new();
            m.insert("key".into(), json!(r.key));
            m.insert("cells".into(), Value::Array(cells));
            put(&mut m, "tone", json!(if r.faded { "dim" } else { "" }));
            Value::Object(m)
        })
        .collect();
    put(&mut m, "rows", Value::Array(rows));
    put(&mut m, "selected", json!(n.selected));
    let stats: Vec<Value> = n
        .stats
        .iter()
        .map(|s| json!({"label": s.label, "value": s.value, "tone": ink(s.ink)}))
        .collect();
    put(&mut m, "stats", Value::Array(stats));
    put(
        &mut m,
        "detail",
        Value::Array(n.detail.iter().map(|(k, v)| json!([k, v])).collect()),
    );
    let actions: Vec<Value> = n
        .actions
        .iter()
        .map(|a| {
            let mut m = Map::new();
            m.insert("id".into(), json!(a.id));
            m.insert("label".into(), json!(a.label));
            m.insert("key".into(), json!(a.key.to_string()));
            put(&mut m, "destructive", json!(a.destructive));
            put(&mut m, "hint", json!(a.hint));
            put(&mut m, "resource", json!(a.resource));
            Value::Object(m)
        })
        .collect();
    put(&mut m, "actions", Value::Array(actions));
    put(&mut m, "text", json!(n.text));
    put(&mut m, "values", json!(n.values));
    put(&mut m, "last", json!(n.last));
    put(&mut m, "missing", json!(n.missing));
    Value::Object(m)
}

/// A golden's node with what the Rust page does not carry taken out: a column's role, and an align that is not
/// right (only right-aligned columns are drawn differently).
fn trim(v: &mut Value) {
    if let Some(cols) = v.get_mut("cols").and_then(Value::as_array_mut) {
        for c in cols.iter_mut().filter_map(Value::as_object_mut) {
            c.remove("role");
            if c.get("align").and_then(Value::as_str) != Some("right") {
                c.remove("align");
            }
        }
    }
    if let Some(kids) = v.get_mut("children").and_then(Value::as_array_mut) {
        kids.iter_mut().for_each(trim);
    }
}

fn check_console(page: &ConsolePage, want: &Value) {
    let mut body = g(want, "/body").clone();
    trim(&mut body);
    assert_eq!(node(&page.body), body, "{}'s {}", page.name, page.tab);
    assert_eq!(page.state, s(g(want, "/state")));
    assert_eq!(page.awake, g(want, "/awake").as_bool().unwrap());
    assert_eq!(page.note, s(g(want, "/note")));
    let keys: Vec<Value> = page.keys.iter().map(|(k, w)| json!([k, w])).collect();
    assert_eq!(Value::Array(keys), *g(want, "/keys"));
}

#[test]
fn module_consoles() {
    let Some((_home, _d, hub, key)) = following("modules") else {
        return;
    };
    let _w = (
        hub.watch_console(&key, "db"),
        hub.watch_console(&key, "events"),
    );
    let st = until(&hub, &key, "the consoles", |s| {
        let rows = |i: &str, p: &str| {
            s.consoles
                .get(i)
                .and_then(|c| c.tables.get(p))
                .map_or(0, Vec::len)
        };
        rows("db", "sessions") == 4
            && rows("events", "topics") == 3
            && rows("events", "groups") == 2
    });
    let sel = console::Selection::new();
    for (inst, file) in [
        ("db", "modules-db-activity"),
        ("events", "modules-events-console"),
    ] {
        let want = golden(file);
        let page = console::console_page(inst, s(g(&want, "/tab")), st.consoles.get(inst), &sel);
        check_console(&page, &want);
    }
    // The stream follows the selected topic.
    let events = st.consoles.get("events").unwrap();
    let (panel, sector) = console::follow(events, "console:_", &sel);
    assert_eq!((panel.as_str(), sector.as_str()), ("tail", "orders"));
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
        g(want, "/recent")
            .as_array()
            .cloned()
            .map_or(json!([]), Value::Array),
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
