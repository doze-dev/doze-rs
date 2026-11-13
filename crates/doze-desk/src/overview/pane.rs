//! The List view's details pane: the selected instance in full, as the Go page drew it from `view.InstanceOf` (its
//! connect rows, why it is awake, what it needs, its last error). Only what the pane shows is computed here; the
//! instance page has its own model.

use std::time::SystemTime;

use doze_control::InstanceView;
use doze_display as words;
use serde::Serialize;

use super::Look;
use crate::view::StackView;

/// The selected instance, for the pane.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Pane {
    pub name: String,
    pub kind: String,
    pub run: String,
    pub look: Look,
    pub because: String,
    /// How long it has been awake: `14m`.
    pub up: String,
    pub pid: i64,
    /// The connect card's rows.
    pub connect: Vec<Kv>,
    /// Why it is awake; `sleeps` is the sentence under them.
    pub holders: Vec<Pill>,
    pub sleeps: String,
    pub needs: Vec<NeedChip>,
    /// Set while it is crashing or has failed.
    pub crash: Option<Crash>,
}

/// One row of a card: a key, a value, what Copy copies.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Kv {
    pub k: String,
    pub v: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub copy: String,
    pub mono: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub dim: bool,
}

/// One reason it is awake, and a word on it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Pill {
    pub text: String,
    pub explain: String,
}

/// One thing it needs, with how that is.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct NeedChip {
    pub name: String,
    pub known: bool,
    pub look: Look,
    /// `✓` once it is up or done.
    pub glyph: &'static str,
    pub task: bool,
}

/// What a crashing or failed instance last said.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Crash {
    pub error: String,
    /// `try 3 of 5`.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub tries: String,
    /// `4 s`.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub next_in: String,
}

/// The pane for the instance `name` of `st`, at `now`; `None` when the stack has none of that name.
pub fn pane(st: &StackView, name: &str, now: SystemTime) -> Option<Pane> {
    let snap = st.words();
    let (v, i) = st
        .instances
        .iter()
        .zip(&snap.instances)
        .find(|(v, _)| v.name == name)?;
    let (holders, sleeps) = holders(v, st.idle_timeout);
    let mut p = Pane {
        name: v.name.clone(),
        kind: v.kind.clone(),
        run: v.run.clone(),
        look: Look::of(words::look(i)),
        because: words::because(i, &snap, now),
        pid: v.pid,
        connect: connect(v, i),
        holders,
        sleeps,
        ..Pane::default()
    };
    if matches!(i.state, words::State::Active | words::State::Idle)
        && let Some(s) = i.started_at
    {
        p.up = words::ago(now.duration_since(s).unwrap_or_default());
    }
    p.needs = words::needs(i, &snap)
        .iter()
        .map(|n| NeedChip {
            name: n.name.to_owned(),
            known: n.known.is_some(),
            look: n.known.map(|k| Look::of(k.look)).unwrap_or_default(),
            glyph: n.glyph().unwrap_or(""),
            task: n.known.is_some_and(|k| k.ran)
                || snap
                    .find(n.name)
                    .is_some_and(|o| o.kind == words::Kind::Task),
        })
        .collect();
    if words::troubled(i) && v.kind != "task" {
        p.crash = Some(Crash {
            error: words::first_line(&v.last_error).to_owned(),
            tries: if v.restart_max > 0 {
                format!("try {} of {}", v.restart_count, v.restart_max)
            } else {
                String::new()
            },
            next_in: i
                .next_restart
                .and_then(|t| t.duration_since(now).ok())
                .map(|d| format!("{} s", (d.as_secs_f64() + 0.5).floor()))
                .unwrap_or_default(),
        });
    }
    Some(p)
}

/// The connect card's rows, as the Instance board lists them (Go `connectRows`).
fn connect(v: &InstanceView, i: &words::Instance) -> Vec<Kv> {
    let mut rows = Vec::new();
    let kv = |k: &str, val: &str, copy: &str, dim: bool| Kv {
        k: k.to_owned(),
        v: val.to_owned(),
        copy: copy.to_owned(),
        mono: true,
        dim,
    };
    if !v.https.is_empty() {
        rows.push(kv("Domain", &v.https, &v.https, false));
        for d in v.domains.iter().filter(|d| d.starts_with("*.")) {
            rows.push(kv(
                "Subdomains",
                &format!("https://{d} · any name under it"),
                "",
                true,
            ));
        }
    }
    if !v.url.is_empty() {
        rows.push(kv("URL", &v.url, &v.url, false));
    }
    if !v.env_var.is_empty() {
        rows.push(kv(
            "Env var",
            &v.env_var,
            &format!("export {}={}", v.env_var, v.url),
            false,
        ));
    }
    if let Some(a) = words::address(i) {
        rows.push(kv("Address", &a, &a, false));
    }
    if !v.data_dir.is_empty() {
        let home = words::home();
        rows.push(kv(
            "Data",
            &words::tilde(&v.data_dir, home.as_deref()),
            "",
            true,
        ));
    }
    rows
}

/// The "why it's awake" pills, and the sentence under them (Go `holders`).
fn holders(v: &InstanceView, idle: std::time::Duration) -> (Vec<Pill>, String) {
    let pill = |text: String, explain: &str| Pill {
        text,
        explain: explain.to_owned(),
    };
    let pills: Vec<Pill> = v
        .holders
        .iter()
        .filter_map(|h| match h.kind.as_str() {
            "dependant" => Some(pill(
                format!("{} needs it", h.name),
                "a dependant that's awake keeps it awake",
            )),
            "connection" => Some(pill(
                format!(
                    "{} connection{}",
                    h.count,
                    words::plural(u64::try_from(h.count).unwrap_or(0))
                ),
                "open clients, through doze's proxy",
            )),
            "pin" => Some(pill(
                "pinned awake".to_owned(),
                "stays up past the idle time until unpinned",
            )),
            _ => None,
        })
        .collect();
    if v.always_on {
        let s = "It runs until stopped: it has no port, so nothing can hold it, and it never sleeps on its own.";
        return (pills, s.to_owned());
    }
    let after = if idle.is_zero() {
        String::new()
    } else {
        format!(" after {}", words::clock(idle))
    };
    let sleeps = if !pills.is_empty() {
        format!("With neither, it sleeps{after}. Its data stays; the next connection wakes it.")
    } else if v.state == "idle" {
        "nothing: it sleeps when the idle time is up".to_owned()
    } else {
        format!(
            "No connection right now. It dozes, then sleeps{after} if nothing connects; the next connection wakes it."
        )
    };
    (pills, sleeps)
}
