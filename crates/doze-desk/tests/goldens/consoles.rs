//! The module consoles' pages against Go's goldens: the db and events consoles of the `modules` fixture, resolved as
//! Go's dash resolved them (`console_page`), with the app's deliberate changes said where they are checked.

use super::check::{g, golden, s};
use super::common::{following, until};
use super::pages::{ink, put};
use doze_desk::page::console::{self, ConsolePage, Node};
use serde_json::{Map, Value, json};

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
    // A deliberate change from Go: a stream with no lines yet is one line saying so, not a block taking the room left.
    if let Some(o) = v.as_object_mut()
        && o.get("kind").and_then(Value::as_str) == Some("stream")
        && o.get("rows")
            .and_then(Value::as_array)
            .is_none_or(Vec::is_empty)
    {
        o.remove("flexible");
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
    // A deliberate change from Go: the note is no longer a frame's header line on every tab ("the module's console ·
    // re-read every 2s while awake; it never wakes db") but a quiet line in the keys bar.
    assert!(s(g(want, "/note")).starts_with("the module's console"));
    assert_eq!(
        page.note,
        format!("re-read every 2s while awake · never wakes {}", page.name)
    );
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
