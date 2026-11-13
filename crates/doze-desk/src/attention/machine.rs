//! The machine's parts not in place, and what stacks left behind (Go `MachineItems`).

use doze_display as words;

use super::{Fix, FixKind, Item, Place};

/// One part of the machine's setup, as doctor reports it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MachinePart {
    /// `names`, `domains`, `app`.
    pub section: String,
    pub name: String,
    pub ok: bool,
    pub text: String,
    /// The command that sets it up, when doctor names one.
    pub fix: String,
}

/// The machine's parts not in place, one item per section, and what stacks left behind (Go `MachineItems`).
#[must_use]
pub fn machine_items(
    parts: &[MachinePart],
    repairable: bool,
    leftovers: usize,
    data: usize,
) -> Vec<Item> {
    let mut sections: Vec<(&str, Vec<&MachinePart>)> = Vec::new();
    for p in parts
        .iter()
        .filter(|p| !p.ok && !p.name.starts_with("stale route "))
    {
        match sections.iter_mut().find(|(s, _)| *s == p.section) {
            Some((_, v)) => v.push(p),
            None => sections.push((&p.section, vec![p])),
        }
    }
    let machine = || {
        Some(Place {
            page: "settings",
            section: "machine",
            ..Place::default()
        })
    };
    let mut out: Vec<Item> = sections
        .into_iter()
        .filter_map(|(sec, cs)| section_item(sec, &cs, repairable, machine()))
        .collect();
    if leftovers > 0 {
        out.push(Item {
            id: "leftovers".into(),
            print: leftovers.to_string(),
            kind: "leftovers",
            title: format!(
                "{leftovers} thing{} left behind",
                words::plural(leftovers as u64)
            ),
            detail: "processes and routes from stacks that went out of step with doze".into(),
            fixes: vec![Fix::new("Clean up", FixKind::Clean).primary()],
            show: machine(),
            ..Item::default()
        });
    }
    if data > 0 {
        // Data is deleted one folder at a time, after asking: in Settings.
        out.push(Item {
            id: "leftdata".into(),
            print: data.to_string(),
            kind: "leftdata",
            title: format!(
                "{data} data folder{} no stack uses",
                words::plural(data as u64)
            ),
            detail: "kept until you look at each one in Settings › Machine".into(),
            show: machine(),
            ..Item::default()
        });
    }
    out
}

fn section_item(
    sec: &str,
    cs: &[&MachinePart],
    repairable: bool,
    show: Option<Place>,
) -> Option<Item> {
    let first = cs.first()?;
    let names: Vec<&str> = cs.iter().map(|c| c.name.as_str()).collect();
    let mut it = Item {
        id: format!("machine:{sec}"),
        print: names.join(","),
        kind: "machine",
        detail: format!("missing: {}", names.join(", ")),
        show,
        ..Item::default()
    };
    it.title = match sec {
        "domains" => {
            it.detail =
                "one admin prompt: a resolver route and the doze certificate authority".into();
            "Your domains are not set up on this Mac".into()
        }
        "names" => ".doze names are not set up on this Mac".into(),
        "app" => {
            it.detail.clear();
            if first.text.is_empty() {
                format!("{} is not in place", first.name)
            } else {
                first.text.clone()
            }
        }
        other => format!("{other} is not in place"),
    };
    if repairable {
        it.fixes
            .push(Fix::new("Set up (one password)", FixKind::Repair).primary());
    } else if !first.fix.is_empty() {
        it.fixes.push(Fix {
            text: first.fix.clone(),
            ..Fix::new("Copy the command", FixKind::Copy).primary()
        });
    }
    Some(it)
}
