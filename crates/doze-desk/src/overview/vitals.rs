//! The strip of figures over an overview (Go `vitalsOf`): memory with each instance's share, cores, the instances by
//! state, connections and the setup tasks.

use doze_display as words;
use serde::Serialize;

use super::part;
use crate::view::StackView;

/// The four figures across the top.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Vitals4 {
    pub mem: Meter,
    pub cpu: Meter,
    pub state: States,
    pub conns: Conns,
}

/// A bar figure: the total, the shares, and the key line under it.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Meter {
    pub total: String,
    pub shares: Vec<Share>,
    pub key: String,
}

/// One instance's part of a meter.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Share {
    pub name: String,
    /// Of the sum, 0 to 1, for the bar's width.
    pub part: f64,
    pub label: String,
}

/// The instances by state, tasks aside, and when the stack sleeps.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct States {
    pub awake: usize,
    pub dozing: usize,
    pub waking: usize,
    pub asleep: usize,
    pub off: usize,
    pub paused: usize,
    pub crashing: usize,
    /// `sleeps after 5:00 with nothing holding it`.
    pub sleeps: String,
}

/// Connections open across the stack, and the tasks.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Conns {
    pub open: u64,
    /// `tasks ✓ 1 done · ▸ 1 not run`.
    pub tasks: String,
}

/// The four figures, from the instances' latest readings (Go `vitalsOf`).
pub(crate) fn vitals(st: &StackView, snap: &words::Snapshot) -> Vitals4 {
    let t = words::tally(&snap.instances);
    let mut v4 = Vitals4 {
        state: States {
            awake: t.awake,
            dozing: t.dozing,
            waking: t.waking,
            asleep: t.asleep,
            off: t.off,
            paused: t.paused,
            crashing: t.trouble,
            sleeps: String::new(),
        },
        conns: Conns {
            open: t.conns,
            tasks: tasks(&snap.instances),
        },
        ..Vitals4::default()
    };
    let mut mem = Vec::new();
    let mut cpu = Vec::new();
    for (v, i) in st.instances.iter().zip(&snap.instances) {
        if i.kind != words::Kind::Task && words::awake_now(i) {
            #[allow(clippy::cast_precision_loss)] // WHY: Go's float64(v.RAM)
            mem.push(words::Share {
                name: &v.name,
                v: v.ram as f64,
            });
            cpu.push(words::Share {
                name: &v.name,
                v: v.cpu,
            });
        }
    }
    v4.mem = mem_meter(mem);
    v4.cpu = cpu_meter(cpu);
    if !st.idle_timeout.is_zero() {
        v4.state.sleeps = format!(
            "sleeps after {} with nothing holding it",
            words::clock(st.idle_timeout)
        );
    }
    v4
}

fn mem_meter(all: Vec<words::Share<'_>>) -> Meter {
    let m = words::shares(all, 0.0);
    let unit = words::mem_unit(m.sum);
    #[allow(clippy::cast_possible_truncation)] // WHY: Go's int64(memSum)
    let total = words::mb(m.sum as i64);
    let mut key: Vec<String> = Vec::new();
    let shares: Vec<Share> = m
        .shares
        .iter()
        .map(|s| Share {
            name: s.name.to_owned(),
            part: part(s.v, m.sum),
            label: unit(s.v),
        })
        .collect();
    for s in shares.iter().take(3) {
        key.push(format!("{} {}", s.name, s.label));
    }
    if shares.len() > 3 {
        key.push(format!("+{} more", shares.len() - 3));
    }
    Meter {
        total,
        shares,
        key: key.join(" · "),
    }
}

fn cpu_meter(all: Vec<words::Share<'_>>) -> Meter {
    let any = !all.is_empty();
    let m = words::shares(all, 0.05);
    let mut total = words::cores(m.sum);
    if m.sum <= 0.0 && any {
        // Running, and too little to measure: not a broken zero.
        total = "<0.01 cores".to_owned();
    }
    let shares: Vec<Share> = m
        .shares
        .iter()
        .map(|s| Share {
            name: s.name.to_owned(),
            part: part(s.v, m.sum),
            label: format!("{:.1}%", s.v),
        })
        .collect();
    let mut key: Vec<String> = shares
        .iter()
        .take(2)
        .map(|s| format!("{} {}", s.name, s.label))
        .collect();
    if !key.is_empty() {
        key.push("the rest quiet".to_owned());
    } else if any {
        key.push("nothing busy right now".to_owned());
    }
    Meter {
        total,
        shares,
        key: key.join(" · "),
    }
}

/// How the setup tasks stand: `tasks ✓ 1 done · ▸ 1 not run`.
fn tasks(ins: &[words::Instance]) -> String {
    let (mut done, mut ready, mut failed) = (0, 0, 0);
    for i in ins.iter().filter(|i| i.kind == words::Kind::Task) {
        match &i.task {
            Some(t) if t.outcome == words::Outcome::Failed => failed += 1,
            Some(_) => done += 1,
            None => ready += 1,
        }
    }
    let mut parts = Vec::new();
    if done > 0 {
        parts.push(format!("✓ {done} done"));
    }
    if failed > 0 {
        parts.push(format!("✗ {failed} failed"));
    }
    if ready > 0 {
        parts.push(format!("▸ {ready} not run"));
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!("tasks {}", parts.join(" · "))
    }
}
