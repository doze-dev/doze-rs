//! The view-models of the Resources pages, from a descriptor and what its calls answer.

use aws_wire::ApiError;

use super::bus::Bus;
use super::run::{self, MOST_COUNTED, PAGES_COUNTED};
use super::{Column, Descriptor, Format, InputKind, Kind, catalog};
use crate::facts::Facts;
use crate::model::{
    CallRecord, ColumnSpec, CreateSpec, DeleteSpec, FormInput, KindCount, ResourceDetail,
    ResourceList, ResourceService, ResourcesIndex,
};
use crate::ring::Recorder;

/// The most calls of the Live record a detail page shows.
pub const MOST_CALLS: usize = 20;

/// How the page shows a format (the server has already written out `Flag` and `Tail`).
#[must_use]
pub const fn format(f: Format) -> crate::model::ValueFormat {
    use crate::model::ValueFormat as V;
    match f {
        Format::Text | Format::Flag(..) => V::Text,
        Format::Mono | Format::Tail => V::Mono,
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

fn create_form(kind: &Kind) -> Option<CreateSpec> {
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
    for _ in 0..PAGES_COUNTED {
        match run::page(bus, d, kind, token.as_deref()).await {
            Ok((items, next)) => {
                for item in &items {
                    let id = super::path::text(item, kind.list.id).unwrap_or_default();
                    let name = super::path::text(item, kind.list.name.path)
                        .map(|n| {
                            if kind.list.name.tail {
                                super::path::tail(&n)
                            } else {
                                n
                            }
                        })
                        .unwrap_or(id);
                    k.names.push(name);
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
}

/// One page of `kind`.
///
/// # Errors
/// The service's refusal.
pub async fn list(
    bus: &Bus,
    facts: &Facts,
    (d, kind): (&Descriptor, &Kind),
    token: Option<&str>,
) -> Result<ResourceList, ApiError> {
    let (items, next) = run::page(bus, d, kind, token).await?;
    let rows = run::rows(bus, d, kind, items).await;
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
        operations.push(sub.step.op.to_owned());
        subs.push(run::sub(bus, d, sub, (&id, name), &out).await);
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
        calls: calls(facts, recorder, d.service, (&id, name)),
        reveal: kind.reveal.as_ref().map(|r| r.label.to_owned()),
        delete: delete_spec(kind),
        operations,
        name: name.to_owned(),
        id,
    })
}

/// The newest calls of the Live record to `service` that acted on the resource, newest first.
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
        .take(MOST_CALLS)
        .map(|c| CallRecord::new(c, facts))
        .collect()
}
