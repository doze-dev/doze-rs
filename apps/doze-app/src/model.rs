//! What the screens show of a stack: computed from the hub's [`StackView`] with the words (`doze-display`), plain
//! data with no GPUI in it. It is `PartialEq`, so a screen re-renders only when what it shows changed.

use std::collections::HashMap;
use std::time::SystemTime;

use doze_desk::StackView;
use doze_display::{self as words, Kind, Look, Tone};

/// One instance as a card or row shows it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Row {
    pub(crate) name: String,
    pub(crate) kind: Kind,
    pub(crate) look: Look,
    /// What it is down to (`held by gateway`, `asleep 3m`).
    pub(crate) because: String,
    /// Its memory, as the column shows it (empty for a task).
    pub(crate) mem: String,
    /// The engine line of a module (`postgres 18`).
    pub(crate) engine: String,
    pub(crate) trouble: bool,
    /// Awake now (Go `display.AwakeNow`): only then does a card say its memory.
    pub(crate) awake: bool,
    /// Pinned awake (`keep_awake`): the card's pin.
    pub(crate) pinned: bool,
}

/// The vitals strip over the page.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Vitals {
    /// `536 MB`.
    pub(crate) mem: String,
    /// The biggest few, `storefront 61 · recommendations 58 · +11 more`.
    pub(crate) shares: String,
    /// Each instance's share of the memory, biggest first: what the share bar draws.
    pub(crate) bar: Vec<f32>,
    /// `0.05 cores`.
    pub(crate) cores: String,
    pub(crate) awake: usize,
    pub(crate) dozing: usize,
    pub(crate) trouble: usize,
    pub(crate) conns: u64,
    /// The setup tasks: `tasks ✓ 9 done · 1 not run`, or empty.
    pub(crate) tasks: String,
}

/// A setup task's mark on the card that needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Fold {
    pub(crate) glyph: &'static str,
    pub(crate) tone: Tone,
    /// Its title (Go `MapFold.Title`): "before it: migrate ✓ 16m ago · seed not run yet".
    pub(crate) title: String,
}

/// The stack page.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Page {
    pub(crate) name: String,
    pub(crate) dir: String,
    /// `up 14m`, or empty when stopped.
    pub(crate) up: String,
    pub(crate) running: bool,
    pub(crate) rows: Vec<Row>,
    /// The Map's input, in display order.
    pub(crate) map: Vec<doze_map::Instance>,
    /// Setup-task marks, by the card that needs the tasks.
    pub(crate) folds: HashMap<String, Fold>,
    pub(crate) vitals: Vitals,
    /// What a small map's detail card says of each instance, beyond the layout.
    pub(crate) said: HashMap<String, crate::map::said::Said>,
}

/// A module's engine and its version (`postgres 17.2`), without a `builtin` prefix, as the words write it.
pub(crate) fn engine_version(i: &words::Instance) -> String {
    let version = i
        .version
        .strip_prefix("builtin")
        .unwrap_or(&i.version)
        .trim();
    format!("{} {version}", i.engine).trim().to_owned()
}

/// The page for a stack, as of `now`.
pub(crate) fn page(v: &StackView, now: SystemTime) -> Page {
    let snap = v.words();
    let rows: Vec<Row> = snap
        .instances
        .iter()
        .map(|i| Row {
            name: i.name.clone(),
            kind: i.kind,
            look: words::look(i),
            because: words::because(i, &snap, now),
            mem: words::mem(i).into_owned(),
            // With its version (`postgres 18`), as the Map's module card says it.
            engine: engine_version(i),
            trouble: words::troubled(i),
            awake: words::awake_now(i),
            pinned: i.keep_awake,
        })
        .collect();
    let up = v
        .since
        .and_then(|s| now.duration_since(s).ok())
        .map(|d| format!("up {}", words::ago(d)))
        .unwrap_or_default();
    Page {
        name: v.name.clone(),
        dir: v.dir.clone(),
        up: if v.running { up } else { String::new() },
        running: v.running,
        map: v.instances.iter().map(map_instance).collect(),
        folds: folds(&snap, now),
        vitals: vitals(v, &snap),
        said: crate::map::said::said_of(&snap, now),
        rows,
    }
}

/// An instance as the Map lays it out: what it calls and uses (`needs`), the language from its command line, and a
/// task that is running or failed drawn as a card of its own.
fn map_instance(v: &doze_control::InstanceView) -> doze_map::Instance {
    let kind = match v.kind.as_str() {
        "app" => doze_map::Kind::App,
        "task" => doze_map::Kind::Task,
        "service" => doze_map::Kind::Service,
        _ => doze_map::Kind::Module,
    };
    let mut i = doze_map::Instance::new(&v.name, kind, &[]);
    i.needs.clone_from(&v.needs);
    i.run.clone_from(&v.run);
    i.engine = v.engine.split_whitespace().next().unwrap_or("").to_owned();
    i.busy = kind == doze_map::Kind::Task
        && (v.state == "booting" || v.task.as_ref().is_some_and(|t| t.result == "failed"));
    if !v.group.is_empty() {
        i.group = Some(v.group.clone());
    }
    i
}

/// The mark each card gets for the setup tasks it needs that are not drawn as cards of their own (Go `flow.fold`; a
/// task that runs or failed is a card): ✓ in good when every one has run, ▸ in dim otherwise, titled with each
/// task's line.
fn folds(snap: &words::Snapshot, now: SystemTime) -> HashMap<String, Fold> {
    let mut out = HashMap::new();
    for v in &snap.instances {
        let mut seen: Vec<&str> = Vec::new();
        let tasks: Vec<&words::Instance> = v
            .needs
            .iter()
            .filter_map(|n| snap.find(n))
            .filter(|t| t.kind == Kind::Task && t.name != v.name)
            .filter(|t| {
                let fresh = !seen.contains(&t.name.as_str());
                seen.push(&t.name);
                fresh
            })
            // Carded: running, or its last run failed (`map_instance`'s `busy`).
            .filter(|t| {
                t.state != words::State::Booting
                    && !t
                        .task
                        .as_ref()
                        .is_some_and(|r| r.outcome == words::Outcome::Failed)
            })
            .collect();
        if tasks.is_empty() {
            continue;
        }
        let ran = tasks.iter().all(|t| t.task.is_some());
        let title = format!(
            "before it: {}",
            tasks
                .iter()
                .map(|t| task_line(t, now))
                .collect::<Vec<_>>()
                .join(" · ")
        );
        let (glyph, tone) = if ran {
            ("✓", Tone::Good)
        } else {
            ("▸", Tone::Dim)
        };
        out.insert(v.name.clone(), Fold { glyph, tone, title });
    }
    out
}

/// A task as its fold's title says it (Go `taskLine`): "migrate ✓ 16m ago".
fn task_line(t: &words::Instance, now: SystemTime) -> String {
    let ago = || {
        t.task
            .as_ref()
            .and_then(|r| r.ran_at)
            .and_then(|a| now.duration_since(a).ok())
            .map(words::ago)
            .unwrap_or_else(|| words::ago(std::time::Duration::ZERO))
    };
    match &t.task {
        _ if t.state == words::State::Booting => format!("{} running", t.name),
        None => format!("{} not run yet", t.name),
        Some(r) if r.outcome == words::Outcome::Failed => {
            format!("{} failed {} ago", t.name, ago())
        }
        Some(_) => format!("{} ✓ {} ago", t.name, ago()),
    }
}

fn vitals(v: &StackView, snap: &words::Snapshot) -> Vitals {
    let t = words::tally(&snap.instances);
    let mut by_mem: Vec<(&str, u64)> = snap
        .instances
        .iter()
        .filter(|i| i.ram > 0 && i.kind != Kind::Task)
        .map(|i| (i.name.as_str(), i.ram))
        .collect();
    by_mem.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    let total: u64 = by_mem.iter().map(|x| x.1).sum();
    let mut shares: Vec<String> = by_mem
        .iter()
        .take(3)
        .map(|(n, b)| format!("{n} {}", mb_figure(*b)))
        .collect();
    if by_mem.len() > 3 {
        shares.push(format!("+{} more", by_mem.len() - 3));
    }
    #[allow(clippy::cast_precision_loss)] // WHY: a share of a bar a few hundred pixels wide
    let bar = by_mem
        .iter()
        .map(|x| x.1 as f32 / total.max(1) as f32)
        .collect();
    let cpu: f64 = v.instances.iter().map(|i| i.cpu).sum();
    Vitals {
        mem: words::mb(i64::try_from(total).unwrap_or(i64::MAX)),
        shares: shares.join(" · "),
        bar,
        cores: words::cores(cpu),
        awake: t.awake,
        dozing: t.dozing,
        trouble: t.trouble,
        conns: t.conns,
        tasks: tasks(&snap.instances),
    }
}

/// How the setup tasks stand, as the strip says it.
fn tasks(ins: &[words::Instance]) -> String {
    let mut n = [0usize; 3];
    for i in ins.iter().filter(|i| i.kind == Kind::Task) {
        let slot = match words::look(i) {
            Look::Done => 0,
            Look::Failed => 2,
            _ => 1,
        };
        if let Some(c) = n.get_mut(slot) {
            *c += 1;
        }
    }
    let [done, not_run, failed] = n;
    let mut parts = Vec::new();
    if done > 0 {
        parts.push(format!(
            "{} {done} {}",
            Look::Done.glyph(),
            Look::Done.word()
        ));
    }
    if failed > 0 {
        parts.push(format!(
            "{} {failed} {}",
            Look::Failed.glyph(),
            Look::Failed.word()
        ));
    }
    if not_run > 0 {
        parts.push(format!("{not_run} {}", Look::NotRun.word()));
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!("tasks {}", parts.join(" · "))
    }
}

/// Megabytes as a bare figure, as the strip writes them (`61`).
fn mb_figure(bytes: u64) -> u64 {
    (bytes + (1 << 19)) >> 20
}

/// The card's state as an index for painting far out: 0 awake, 1 dozing, 2 asleep or off, 3 waking, 4 in trouble.
pub(crate) fn state_index(r: Option<&Row>) -> u8 {
    match r {
        None => 2,
        Some(r) if r.trouble => 4,
        Some(r) => match r.look {
            Look::Awake => 0,
            Look::Dozing => 1,
            Look::Waking => 3,
            _ => 2,
        },
    }
}

#[cfg(test)]
mod tests;
