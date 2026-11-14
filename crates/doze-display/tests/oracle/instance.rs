//! A snapshot's cases: everything said about each instance, the listing, the tree and the tally; and the journal's.

use doze_display as d;
use doze_display::{Group, Instance, Look, Snapshot};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::input::{EventIn, SnapIn, TaskIn};
use crate::{HOME, nanos, now};

fn look(l: Option<Look>) -> Value {
    l.map_or_else(
        || json!(["", "", ""]),
        |l| json!([l.word(), l.glyph(), l.tone().as_str()]),
    )
}

pub(crate) fn snapshot(input: &Value) -> Value {
    let snap = SnapIn::deserialize(input).expect("a snapshot").snapshot();
    let each: Vec<Value> = snap.instances.iter().map(|v| instance(v, &snap)).collect();
    let rows = d::rows(&snap);
    let group = |g: Group| Group::ALL.iter().position(|x| *x == g).expect("a group");
    let listed: Vec<Value> = rows
        .iter()
        .map(|r| json!([r.v.name, group(r.group)]))
        .collect();
    let (ordered, depth) = d::tree_order(&rows);
    let tree: Vec<Value> = ordered
        .iter()
        .map(|r| json!([r.v.name, depth[r.v.name.as_str()]]))
        .collect();
    let t = d::tally(&snap.instances);
    json!({
        "each": each, "rows": listed, "tree": tree,
        "tally": [t.awake, t.dozing, t.waking, t.asleep, t.off, t.paused, t.trouble, t.failed, t.ram, t.conns],
    })
}

fn instance(v: &Instance, snap: &Snapshot) -> Value {
    let now = now();
    let url = d::reach(v).map_or_else(
        || json!(["", ""]),
        |r| json!([r.addr, r.port.unwrap_or("")]),
    );
    let needs: Vec<Value> = d::needs(v, snap)
        .iter()
        .map(|n| {
            let ran = n.known.is_some_and(|k| k.ran);
            json!([
                n.name,
                n.known.is_some(),
                look(n.known.map(|k| k.look)),
                n.glyph().unwrap_or(""),
                ran
            ])
        })
        .collect();
    let note = d::note(v, snap.idle_timeout, now);
    let held: Vec<Value> = d::held_sentence(v)
        .iter()
        .map(|s| json!([s.text, s.strong]))
        .collect();
    let facts: Vec<Value> = d::reset_facts(v, Some(HOME))
        .iter()
        .map(|f| json!([f.key, f.text, f.tone.as_str()]))
        .collect();
    let tabs: Vec<Value> = d::tabs(v, None)
        .iter()
        .map(|t| json!([t.id, t.title]))
        .collect();
    let mut c = v.clone();
    d::clean_instance(&mut c);
    let why = c.task.as_ref().map_or("", |t| t.why.as_str());
    let cleaned = json!([c.name, c.run, c.url, c.domain, c.last_error, why]);
    let result = v.task.as_ref().map_or(Value::Null, |t| {
        let said = d::task_result(&v.name, t);
        json!([said.text, said.bad])
    });
    json!({
        "word": v.state.word(), "awake": v.state.awake(), "look": look(Some(d::look(v))),
        "awake_now": d::awake_now(v), "in_use": d::in_use(v).unwrap_or_default(), "troubled": d::troubled(v),
        "what": d::what(v, snap, now), "because": d::because(v, snap, now), "runs": d::runs(v),
        "first_unmet": d::first_unmet(v, snap).unwrap_or(""), "holder_named": d::holder_named(v).unwrap_or(""),
        "mem": d::mem(v), "url": url, "needs": needs, "address": d::address(v).unwrap_or_default(),
        "address_of": d::address_of(v), "hint": d::hint(v, snap, now).unwrap_or_default(),
        "switch_on": d::switch_on(v), "note": [note.text, note.bad], "state_rank": d::state_rank(v), "tabs": tabs,
        "has_web": d::has_web(v), "web_url": d::web_url(v), "dependants": d::dependants(v),
        "connections": d::connections(v), "sleep_buttons": d::sleep_buttons(v),
        "restart_buttons": d::restart_buttons(v), "held": held, "reset_facts": facts, "clean_view": cleaned,
        "task_result": result, "short_run": d::short_run(&v.run),
    })
}

#[derive(Deserialize)]
struct HistoryIn {
    #[serde(default)]
    events: Vec<EventIn>,
    last: Option<TaskIn>,
}

pub(crate) fn journal(case: &str, input: &Value) -> Value {
    if case == "event" {
        let e = EventIn::deserialize(input).expect("an event").event();
        let mut cleaned = e.clone();
        d::clean_event(&mut cleaned);
        return json!({"sentence": d::sentence(&e), "cause_words": d::cause_words(&e.cause), "clean": cleaned.detail});
    }
    let h = HistoryIn::deserialize(input).expect("a history");
    let events: Vec<_> = h.events.iter().map(EventIn::event).collect();
    let last = h.last.as_ref().map(TaskIn::record);
    // A run's time is one of the input's, written back as the oracle wrote it: a system clock coarser than a
    // nanosecond (Windows counts 100 ns) cannot hold Go's `...999` and would round it.
    let written: Vec<i64> = input["events"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|e| e["time"].as_i64())
        .chain(input["last"]["ran_at"].as_i64())
        .collect();
    let as_written = |t| {
        written
            .iter()
            .copied()
            .find(|ns| crate::time(*ns) == t)
            .unwrap_or_else(|| nanos(t))
    };
    let runs: Vec<Value> = d::run_history(&events, "migrate", last.as_ref())
        .iter()
        .map(|r| {
            json!([
                as_written(r.at),
                r.why,
                i64::try_from(r.took.as_nanos()).expect("fits"),
                r.ok,
                r.done
            ])
        })
        .collect();
    json!(runs)
}
