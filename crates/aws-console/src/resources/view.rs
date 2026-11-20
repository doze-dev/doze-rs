//! The view-models of the Resources pages, from a descriptor and what its calls answer.

use aws_wire::ApiError;

use super::bus::{Bus, CONSOLE};
use super::run::{self, MOST_COUNTED, PAGES_COUNTED};
use super::{Column, Descriptor, Format, InputKind, Kind, catalog};
use crate::facts::Facts;
use crate::model::{
    CallRecord, ColumnSpec, CreateSpec, DeleteSpec, FormInput, KindCount, ResourceDetail,
    ResourceList, ResourceService, ResourcesIndex, SetAside,
};
use crate::ring::Recorder;

/// The most calls of the Live record a detail page shows.
pub const MOST_CALLS: usize = 20;

/// How the page shows a format (the server has already written out `Flag` and `Tail`).
#[must_use]
pub const fn format(f: Format) -> crate::model::ValueFormat {
    use crate::model::ValueFormat as V;
    match f {
        Format::Text | Format::Flag(..) | Format::Named(_) => V::Text,
        Format::Mono | Format::Tail | Format::Pairs => V::Mono,
        Format::Number => V::Number,
        Format::Bytes => V::Bytes,
        Format::Time => V::Time,
        Format::Seconds => V::Seconds,
        Format::Json => V::Json,
    }
}

/// The columns, as the page reads them.
#[must_use]
pub fn columns(columns: &[Column]) -> Vec<ColumnSpec> {
    columns
        .iter()
        .map(|c| ColumnSpec {
            title: c.title.to_owned(),
            format: format(c.format),
        })
        .collect()
}

pub(super) fn create_form(kind: &Kind) -> Option<CreateSpec> {
    let create = kind.create.as_ref()?;
    let inputs = create
        .inputs
        .iter()
        .map(|i| {
            let (input, options) = match i.kind {
                InputKind::Text => ("text", &[][..]),
                InputKind::Number => ("number", &[][..]),
                InputKind::Textarea => ("textarea", &[][..]),
                InputKind::Select(o) => ("select", o),
                InputKind::Toggle => ("toggle", &[][..]),
            };
            FormInput {
                key: i.key.to_owned(),
                label: i.label.to_owned(),
                help: i.help.to_owned(),
                input: input.to_owned(),
                required: i.required,
                placeholder: i.placeholder.to_owned(),
                value: i.value.to_owned(),
                options: options.iter().map(|o| (*o).to_owned()).collect(),
            }
        })
        .collect();
    Some(CreateSpec {
        operations: create.steps.iter().map(|s| s.op.to_owned()).collect(),
        inputs,
    })
}

/// What deleting one of `kind` does.
#[must_use]
pub fn delete_spec(kind: &Kind) -> Option<DeleteSpec> {
    kind.delete.as_ref().map(|d| DeleteSpec {
        verb: d.verb.to_owned(),
        operation: d.step.op.to_owned(),
        words: d.words.to_owned(),
    })
}

/// The title of `service` in `facts`, else its name.
fn title_of(facts: &Facts, service: &str) -> String {
    facts
        .services
        .iter()
        .find(|s| s.name == service)
        .map_or_else(|| service.to_owned(), |s| s.title.to_owned())
}

/// Whether this run serves `service`.
#[must_use]
pub fn served(facts: &Facts, service: &str) -> bool {
    facts.services.iter().any(|s| s.name == service && s.served)
}

/// The index: every service of the build, each described kind counted by listing it (a served service only, and only when
/// `counted`).
pub async fn index(bus: &Bus, facts: &Facts, counted: bool) -> ResourcesIndex {
    let mut services = Vec::new();
    for entry in catalog::ALL {
        let Some(svc) = facts.services.iter().find(|s| s.name == entry.service) else {
            continue;
        };
        let mut kinds = Vec::new();
        for kind in entry.descriptor.map_or(&[][..], |d| d.kinds) {
            let mut k = KindCount {
                id: kind.id.to_owned(),
                title: kind.title.to_owned(),
                one: kind.one.to_owned(),
                members: kind.members.iter().map(|m| (*m).to_owned()).collect(),
                count: None,
                more: false,
                names: Vec::new(),
                error: None,
                others: None,
            };
            if let (true, true, Some(d)) = (counted, svc.served, entry.descriptor) {
                count(bus, d, kind, &mut k).await;
            }
            kinds.push(k);
        }
        services.push(ResourceService {
            name: svc.name.to_owned(),
            title: svc.title.to_owned(),
            group: entry.group.to_owned(),
            served: svc.served,
            described: entry.descriptor.is_some(),
            note: (!entry.note.is_empty()).then(|| entry.note.to_owned()),
            kinds,
        });
    }
    ResourcesIndex { services }
}

/// Counts `kind` by reading its pages, up to [`MOST_COUNTED`] names or [`PAGES_COUNTED`] pages.
async fn count(bus: &Bus, d: &Descriptor, kind: &Kind, k: &mut KindCount) {
    let mut token: Option<String> = None;
    let mut others = 0;
    for _ in 0..PAGES_COUNTED {
        match run::page(bus, d, kind, token.as_deref()).await {
            Ok((items, next)) => {
                if kind.list.split.is_some() {
                    // What sets a row aside is read by its own calls (a key's manager): made for these rows only.
                    let (rows, set_aside) = run::rows(bus, (d, kind), items, false).await;
                    others += set_aside;
                    k.names.extend(rows.into_iter().map(|r| r.name));
                } else {
                    for item in &items {
                        let id = super::path::text(item, kind.list.id).unwrap_or_default();
                        k.names.push(kind.list.name.read(item).unwrap_or(id));
                    }
                }
                token = next;
            }
            Err(e) => {
                k.error = Some(e.message);
                return;
            }
        }
        if token.is_none() || k.names.len() >= MOST_COUNTED {
            break;
        }
    }
    k.more = token.is_some();
    k.names.truncate(MOST_COUNTED);
    k.count = u32::try_from(k.names.len()).ok();
    if kind.fields.is_empty() {
        // Nothing to open (a metric): the palette is not given them.
        k.names.clear();
    }
    k.others = set_aside(kind, others);
}

/// What `kind` sets aside, `count` of them.
fn set_aside(kind: &Kind, count: u32) -> Option<SetAside> {
    kind.list.split.as_ref().map(|s| SetAside {
        what: s.others.to_owned(),
        count,
    })
}

/// One page of `kind`; with `all`, the rows it sets aside too.
///
/// # Errors
/// The service's refusal.
pub async fn list(
    bus: &Bus,
    facts: &Facts,
    (d, kind): (&Descriptor, &Kind),
    (token, all): (Option<&str>, bool),
) -> Result<ResourceList, ApiError> {
    let (items, next) = run::page(bus, d, kind, token).await?;
    let (rows, others) = run::rows(bus, (d, kind), items, all).await;
    Ok(ResourceList {
        service: d.service.to_owned(),
        service_title: title_of(facts, d.service),
        group: group_of(d.service),
        kind: kind.id.to_owned(),
        title: kind.title.to_owned(),
        one: kind.one.to_owned(),
        about: kind.about.to_owned(),
        operation: kind.list.step.op.to_owned(),
        columns: columns(kind.columns),
        rows,
        next,
        create: create_form(kind),
        delete: delete_spec(kind),
        openable: !kind.fields.is_empty(),
        others: set_aside(kind, others),
        all: all && kind.list.split.is_some(),
        create_elsewhere: (!kind.elsewhere.is_empty()).then(|| kind.elsewhere.to_owned()),
    })
}

fn group_of(service: &str) -> String {
    catalog::ALL
        .iter()
        .find(|e| e.service == service)
        .map_or_else(String::new, |e| e.group.to_owned())
}

/// One resource: its fields, its lists, and its recent calls in the Live record.
///
/// # Errors
/// The service's refusal (no such resource).
pub async fn detail(
    bus: &Bus,
    (facts, recorder): (&Facts, &Recorder),
    (d, kind): (&Descriptor, &Kind),
    name: &str,
) -> Result<ResourceDetail, ApiError> {
    let (id, mut out) = run::id_of(bus, d, kind, name).await?;
    let mut operations: Vec<String> = match &kind.id_of {
        super::Id::Call(step, _) => vec![step.op.to_owned()],
        _ => Vec::new(),
    };
    operations.extend(run::read_detail(bus, d, kind, (&id, name), &mut out).await?);
    let mut subs = Vec::new();
    for sub in kind.subs {
        let mut list = run::sub(bus, d, sub, (&id, name), &out).await;
        // A list read from the detail's answers made no call of its own.
        if !sub.step.is_answered() {
            operations.push(sub.step.op.to_owned());
        }
        list.add = sub.add.and_then(|(of, key)| {
            let other = d.kinds.iter().find(|k| k.id == of)?;
            Some(crate::model::SubAdd {
                kind: of.to_owned(),
                one: other.one.to_owned(),
                create: create_form(other)?,
                key: key.to_owned(),
                value: id.clone(),
            })
        });
        subs.push(list);
    }
    Ok(ResourceDetail {
        service: d.service.to_owned(),
        service_title: title_of(facts, d.service),
        group: group_of(d.service),
        kind: kind.id.to_owned(),
        kind_title: kind.title.to_owned(),
        one: kind.one.to_owned(),
        fields: run::fields(kind, &out),
        subs,
        calls: calls(facts, recorder, d.service, (&id, &called_as(kind, name))),
        reveal: kind.reveal.as_ref().map(|r| r.label.to_owned()),
        delete: delete_spec(kind),
        deep: super::deep::tabs(kind),
        page: crate::deep::page_of(d.service, kind.id).map(str::to_owned),
        operations,
        name: name.to_owned(),
        id,
    })
}

/// What the calls of a resource named `name` name it as: a rule named with its bus (`orders/on-order`) is `on-order` in a
/// call (with the bus apart), as a schedule is without its group. A call to a rule of that name on another bus is counted too.
fn called_as(kind: &Kind, name: &str) -> String {
    let parted = kind.detail.iter().any(|s| s.body.contains("{name:last}"));
    match name.rsplit_once('/') {
        Some((_, last)) if parted => last.to_owned(),
        _ => name.to_owned(),
    }
}

/// The newest calls of the Live record to `service` that acted on the resource, newest first: up to [`MOST_CALLS`] of others'
/// and up to as many of the console's own (a page hides those unless asked).
#[must_use]
pub fn calls(
    facts: &Facts,
    recorder: &Recorder,
    service: &str,
    (id, name): (&str, &str),
) -> Vec<CallRecord> {
    let page = recorder.newest(usize::MAX);
    page.calls
        .iter()
        .rev()
        .filter(|c| facts.service_named(c.signing_name) == service && !c.resource.is_empty())
        .filter(|c| {
            let r: &str = &c.resource;
            r == id
                || r == name
                || super::path::tail(r) == name
                // An S3 path names its bucket first.
                || (c.member.is_empty() && r.split('/').next() == Some(name))
        })
        .scan((0, 0), |(mine, theirs), c| {
            let n = if *c.caller == *CONSOLE { mine } else { theirs };
            *n += 1;
            Some((*n <= MOST_CALLS).then_some(c))
        })
        .flatten()
        .map(|c| CallRecord::new(c, facts))
        .collect()
}
