//! Checks of the hub's state against parts of the Go view goldens.

use doze_desk::{StackView, convert};
use doze_display as words;
use serde_json::Value;
use std::path::Path;

pub(crate) fn golden(name: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("testdata/go/view/{name}.json"));
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// The value at a JSON pointer, or null.
pub(crate) fn g<'a>(v: &'a Value, path: &str) -> &'a Value {
    v.pointer(path).unwrap_or(&Value::Null)
}

pub(crate) fn s(v: &Value) -> &str {
    v.as_str().unwrap_or_default()
}

/// A home page's rows: each instance's state word and meters, as the hub folded them.
pub(crate) fn check_rows(st: &StackView, home: &Value) {
    let rows: Vec<&Value> = g(home, "/groups")
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|grp| g(grp, "/rows").as_array().unwrap())
        .collect();
    assert_eq!(rows.len(), st.instances.len(), "a row for each instance");
    for row in rows {
        let name = s(g(row, "/name"));
        let v = st
            .instance(name)
            .unwrap_or_else(|| panic!("{name} is not in the hub"));
        assert_eq!(
            words::look(&convert::instance(v)).word(),
            s(g(row, "/look/word")),
            "{name}'s look"
        );
        if !g(row, "/mem").is_null() {
            assert_eq!(words::mb(v.ram), s(g(row, "/mem")), "{name}'s memory");
            assert_eq!(format!("{:.1}", v.cpu), s(g(row, "/cpu")), "{name}'s cpu");
        }
    }
    let services: Vec<_> = st.instances.iter().filter(|v| v.kind != "task").collect();
    assert_eq!(g(home, "/total").as_u64(), Some(services.len() as u64));
    let awake = services
        .iter()
        .filter(|v| words::awake_now(&convert::instance(v)))
        .count();
    assert_eq!(g(home, "/awake").as_u64(), Some(awake as u64));
}

fn points(v: &Value) -> Vec<f64> {
    g(v, "/points")
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p.as_f64().unwrap())
        .collect()
}

/// An instance page's charts are the hub's series, point for point.
pub(crate) fn check_series(st: &StackView, page: &Value) {
    let name = s(g(page, "/name"));
    let sr = st.series.get(name).unwrap();
    assert_eq!(points(g(page, "/vitals/ram")), sr.ram, "{name}'s memory");
    assert_eq!(points(g(page, "/vitals/cpu")), sr.cpu, "{name}'s cpu");
    assert_eq!(
        points(g(page, "/vitals/conns")),
        sr.conns,
        "{name}'s connections"
    );
    let v = st.instance(name).unwrap();
    assert_eq!(g(page, "/pid").as_i64(), Some(v.pid));
    assert_eq!(
        words::look(&convert::instance(v)).word(),
        s(g(page, "/look/word"))
    );
}

/// A console table's cells, as the words write them from the rows the hub folded.
pub(crate) fn check_table(st: &StackView, inst: &str, block: &Value) {
    let id = s(g(block, "/id"));
    let panel = id.strip_prefix("table:").unwrap_or(id);
    let c = st.consoles.get(inst).unwrap();
    let spec = c.words.as_deref().unwrap();
    let p = spec
        .panel(panel)
        .unwrap_or_else(|| panic!("{inst} has no panel {panel}"));
    let rows = c.tables.get(panel).unwrap();
    let want: Vec<Vec<&str>> = g(block, "/rows")
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            g(r, "/cells")
                .as_array()
                .unwrap()
                .iter()
                .map(|c| s(g(c, "/text")))
                .collect()
        })
        .collect();
    // The page's columns: the spec's, in the page's order (a session's query goes to the detail block instead), and
    // the "from" column the page adds, written from the `_runtime` cell the hub keeps.
    let mut taken: Vec<&str> = Vec::new();
    let cols: Vec<words::Column> = g(block, "/cols")
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            let key = s(g(c, "/key"));
            let nth = taken.iter().filter(|k| **k == key).count();
            taken.push(key);
            p.columns
                .iter()
                .filter(|x| x.key == key)
                .nth(nth)
                .cloned()
                .unwrap_or(words::Column {
                    key: key.to_owned(),
                    title: s(g(c, "/title")).to_owned(),
                    role: words::Role::Text,
                })
        })
        .collect();
    let got: Vec<Vec<String>> = rows
        .iter()
        .map(|r| {
            cols.iter()
                .map(|col| words::cell(col, r).0.into_owned())
                .collect()
        })
        .collect();
    assert_eq!(got, want, "{inst}'s {panel}");
}
