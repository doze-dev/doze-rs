//! While the window was closed (Go `view/away.go`, the WhileAway board): crashes, failed starts and task runs since
//! the window closed, with how each stands now. Routine wakes and sleeps are not news. `None` when nothing happened.

use std::collections::BTreeMap;
use std::time::{Duration, SystemTime};

use doze_display::{self as words, Kind, Outcome};
use serde::Serialize;

use crate::page::time::{Zone, tenths};
use crate::view::StackView;

/// What happened while the window was closed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Away {
    /// How long the window was closed: "3 h 12 m".
    pub r#for: String,
    pub events: Vec<AwayEvent>,
}

/// One line: who, what, when, and how it is now.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct AwayEvent {
    pub key: String,
    pub stack: String,
    pub instance: String,
    /// The times: "13:02, 13:40".
    pub at: String,
    /// The rest of the line after the name: "crashed twice in shop and started again".
    pub what: String,
    /// How it stands: "still failing · in Needs you", "running", "done".
    pub now: String,
    /// `bad`, `good` or `dim`.
    pub tone: &'static str,
    #[serde(skip)]
    when: Option<SystemTime>,
}

/// What happened in `stacks` between `since` (the window closed) and `now`, newest first, six at most.
#[must_use]
pub fn away_of(
    stacks: &[StackView],
    since: SystemTime,
    now: SystemTime,
    zone: Zone,
) -> Option<Away> {
    if now.duration_since(since).unwrap_or_default() < Duration::from_secs(30) {
        return None;
    }
    let mut out = Vec::new();
    for st in stacks {
        let name = if st.name.is_empty() {
            st.dir
                .trim_end_matches('/')
                .rsplit('/')
                .next()
                .unwrap_or_default()
                .to_owned()
        } else {
            st.name.clone()
        };
        out.extend(troubles(st, &name, since, zone));
        out.extend(task_runs(st, &name, since, zone));
    }
    if out.is_empty() {
        return None;
    }
    out.sort_by_key(|e| std::cmp::Reverse(e.when));
    out.truncate(6);
    Some(Away {
        r#for: span(now.duration_since(since).unwrap_or_default()),
        events: out,
    })
}

/// Crashes and failed starts since, one line per instance.
fn troubles(st: &StackView, name: &str, since: SystemTime, zone: Zone) -> Vec<AwayEvent> {
    let mut by: BTreeMap<&str, Vec<(SystemTime, bool)>> = BTreeMap::new();
    for e in st.events.iter() {
        let Some(t) = e.time.to_system_time().filter(|t| *t >= since) else {
            continue;
        };
        match e.kind.as_str() {
            "crash" => by.entry(&e.instance).or_default().push((t, true)),
            "failed" | "give_up" => by.entry(&e.instance).or_default().push((t, false)),
            _ => {}
        }
    }
    let snap = st.words();
    by.into_iter()
        .map(|(inst, es)| {
            let v = snap.find(inst);
            let troubled = v.is_some_and(words::troubled);
            let crashes = es.iter().filter(|e| e.1).count();
            let failed = es.iter().any(|e| !e.1);
            let at: Vec<String> = es.iter().take(3).map(|e| hm(zone, e.0)).collect();
            let mut ev = AwayEvent {
                key: st.key.dir().to_string_lossy().into_owned(),
                stack: name.to_owned(),
                instance: inst.to_owned(),
                at: at.join(", "),
                when: es.last().map(|e| e.0),
                ..AwayEvent::default()
            };
            ev.what = match crashes {
                0 if failed => format!("did not start in {name}"),
                1 => format!("crashed in {name}"),
                2 => format!("crashed twice in {name}"),
                n => format!("crashed {n} times in {name}"),
            };
            if troubled {
                (ev.now, ev.tone) = ("still failing · in Needs you".into(), "bad");
            } else {
                if crashes > 0 {
                    ev.what.push_str(" and started again");
                }
                match v.filter(|v| !words::awake_now(v)) {
                    Some(v) => (ev.now, ev.tone) = (words::look(v).word().to_owned(), "dim"),
                    None => (ev.now, ev.tone) = ("running".into(), "good"),
                }
            }
            ev
        })
        .collect()
}

/// Task runs since.
fn task_runs(st: &StackView, name: &str, since: SystemTime, zone: Zone) -> Vec<AwayEvent> {
    let snap = st.words();
    snap.instances
        .iter()
        .filter(|v| v.kind == Kind::Task)
        .filter_map(|v| {
            let t = v.task.as_ref()?;
            let ran = t.ran_at.filter(|r| *r >= since)?;
            let failed = t.outcome == Outcome::Failed;
            Some(AwayEvent {
                key: st.key.dir().to_string_lossy().into_owned(),
                stack: name.to_owned(),
                instance: v.name.clone(),
                at: hm(zone, ran),
                what: if failed {
                    format!("failed in {name} · exit {}", t.exit_code)
                } else {
                    format!("ran in {name} · exit 0 in {}", tenths(t.took))
                },
                now: if failed {
                    "still failing · in Needs you"
                } else {
                    "done"
                }
                .into(),
                tone: if failed { "bad" } else { "good" },
                when: Some(ran),
            })
        })
        .collect()
}

/// `15:04`.
fn hm(zone: Zone, t: SystemTime) -> String {
    zone.clock(t).get(..5).unwrap_or_default().to_owned()
}

/// A duration as a person says it: "40 m", "3 h 12 m", "a day", "2 days".
#[must_use]
pub fn span(d: Duration) -> String {
    let m = d.as_secs() / 60;
    match m {
        0..60 => format!("{m} m"),
        60..1440 if m.is_multiple_of(60) => format!("{} h", m / 60),
        60..1440 => format!("{} h {} m", m / 60, m % 60),
        1440..2880 => "a day".into(),
        _ => format!("{} days", m / 1440),
    }
}
