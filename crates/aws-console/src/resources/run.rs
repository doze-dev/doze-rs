//! A descriptor at work: its calls made through [`Bus`], their answers read into the view-models of the Resources pages.

use aws_wire::ApiError;
use serde_json::{Map, Value, json};

use super::bus::Bus;
use super::path::{self, Vars};
use super::{Descriptor, Id, Kind, LinkTo, Step, Sub};
use crate::model::{FieldValue, ResourceLink, ResourceRow, SubList};

/// The most of one kind the index counts: past it, it says "at least".
pub const MOST_COUNTED: usize = 1000;
/// The most pages the index reads for one count.
pub const PAGES_COUNTED: usize = 20;
/// The most rows of one page that [`Kind::each`] is called for.
pub const MOST_EACH: usize = 200;

/// What a body's placeholders are filled from.
#[derive(Debug)]
pub struct Fill<'a> {
    pub bus: &'a Bus,
    pub id: &'a str,
    pub name: &'a str,
    pub token: Option<&'a str>,
    /// What the steps before answered.
    pub out: &'a Value,
    pub form: &'a Map<String, Value>,
}

impl Vars for Fill<'_> {
    fn var(&self, name: &str) -> Option<String> {
        match name {
            "id" => Some(self.id.to_owned()),
            "name" => Some(self.name.to_owned()),
            // A name that holds its parent's (`orders-bus/on-order`, `default/nightly`): the parent, and the rest.
            "name:head" => self.name.rsplit_once('/').map(|(h, _)| h.to_owned()),
            "name:last" => Some(
                self.name
                    .rsplit_once('/')
                    .map_or(self.name, |(_, l)| l)
                    .to_owned(),
            ),
            "account" => Some(self.bus.account().to_owned()),
            "region" => Some(self.bus.region().to_owned()),
            "token" => self.token.map(str::to_owned),
            _ => {
                if let Some(p) = name.strip_prefix("out:") {
                    return path::text(self.out, p);
                }
                let key = name.strip_prefix("form:")?;
                match self.form.get(key)? {
                    Value::String(s) => Some(s.clone()),
                    Value::Bool(true) => Some("true".to_owned()),
                    Value::Number(n) => Some(n.to_string()),
                    _ => None,
                }
            }
        }
    }
}

impl<'a> Fill<'a> {
    /// Nothing but the bus: a list's first page.
    #[must_use]
    pub fn bare(bus: &'a Bus, out: &'a Value, form: &'a Map<String, Value>) -> Self {
        Self {
            bus,
            id: "",
            name: "",
            token: None,
            out,
            form,
        }
    }
}

/// The body of `step` with its placeholders filled.
///
/// # Errors
/// A descriptor whose body is not JSON (a fault of the build).
pub fn body(step: &Step, fill: &Fill<'_>) -> Result<Value, ApiError> {
    let template: Value = serde_json::from_str(step.body)
        .map_err(|e| ApiError::internal(format!("a descriptor's body is not JSON: {e}")))?;
    Ok(path::fill(&template, fill))
}

/// Makes `step` for descriptor `d` with its placeholders filled.
///
/// # Errors
/// The service's refusal.
pub async fn call(d: &Descriptor, step: &Step, fill: &Fill<'_>) -> Result<Value, ApiError> {
    let body = body(step, fill)?;
    fill.bus
        .call(step.service.unwrap_or(d.target), step.op, &body)
        .await
}

/// Every member of `answer` put into `out`.
pub fn merge(out: &mut Value, answer: Value) {
    if let (Value::Object(out), Value::Object(answer)) = (out, answer) {
        out.extend(answer);
    }
}

/// One page of `kind`: its rows as the answer gives them (each an object), and the next page's token. `applied` are the members
/// the list's parameters ([`super::params`]) set over the call's body.
///
/// # Errors
/// The service's refusal.
pub async fn page(
    bus: &Bus,
    d: &Descriptor,
    kind: &Kind,
    token: Option<&str>,
    applied: &Map<String, Value>,
) -> Result<(Vec<Value>, Option<String>), ApiError> {
    let (out, form) = (Value::Null, Map::new());
    let fill = Fill {
        token,
        ..Fill::bare(bus, &out, &form)
    };
    let mut body = body(&kind.list.step, &fill)?;
    if let Value::Object(m) = &mut body {
        m.extend(applied.iter().map(|(k, v)| (k.clone(), v.clone())));
    }
    let answer = bus
        .call(
            kind.list.step.service.unwrap_or(d.target),
            kind.list.step.op,
            &body,
        )
        .await?;
    let items = items_of(&answer, kind.list.items, kind.list.item_as);
    let next = (!kind.list.token_out.is_empty())
        .then(|| path::text(&answer, kind.list.token_out))
        .flatten()
        .filter(|t| !t.is_empty());
    Ok((items, next))
}

/// The list at `items` in `answer`, each a bare value made an object with the one member `item_as`.
pub fn items_of(answer: &Value, items: &str, item_as: &str) -> Vec<Value> {
    let Some(Value::Array(list)) = path::get(answer, items) else {
        return Vec::new();
    };
    list.into_iter()
        .map(|item| match item {
            Value::Object(_) => item,
            bare => json!({ item_as: bare }),
        })
        .collect()
}

/// The rows of one page, with [`Kind::each`] called for each (a failed call leaves its cells empty); the rows the kind sets
/// aside ([`super::Split`]) only when `all`. Also how many it set aside.
pub async fn rows(
    bus: &Bus,
    (d, kind): (&Descriptor, &Kind),
    items: Vec<Value>,
    all: bool,
) -> (Vec<ResourceRow>, u32) {
    let mut rows = Vec::with_capacity(items.len());
    let mut others = 0;
    for (i, mut item) in items.into_iter().enumerate() {
        let id = path::text(&item, kind.list.id).unwrap_or_default();
        let name = kind.list.name.read(&item).unwrap_or_else(|| id.clone());
        if i < MOST_EACH {
            for step in kind.each {
                let form = Map::new();
                let snapshot = item.clone();
                let fill = Fill {
                    id: &id,
                    name: &name,
                    ..Fill::bare(bus, &snapshot, &form)
                };
                if let Ok(answer) = call(d, step, &fill).await {
                    merge(&mut item, answer);
                }
            }
        }
        if let Some(split) = &kind.list.split
            && path::text(&item, split.path).is_none_or(|v| v != split.keep)
        {
            others += 1;
            if !all {
                continue;
            }
        }
        let label = kind
            .list
            .label
            .and_then(|l| l.read(&item))
            .unwrap_or_else(|| name.clone());
        rows.push(ResourceRow {
            cells: kind
                .columns
                .iter()
                .map(|c| path::shown(&item, c.path, c.format))
                .collect(),
            link: None,
            name,
            label,
            id,
        });
    }
    (rows, others)
}

/// What the calls of a resource named `name` take, and what was answered on the way (SQS's `GetQueueUrl`).
///
/// # Errors
/// The service's refusal (no such resource).
pub async fn id_of(
    bus: &Bus,
    d: &Descriptor,
    kind: &Kind,
    name: &str,
) -> Result<(String, Value), ApiError> {
    let mut out = json!({ "$name": name });
    let form = Map::new();
    let id = match &kind.id_of {
        Id::Name => name.to_owned(),
        Id::Template(t) => {
            let snapshot = out.clone();
            let fill = Fill {
                name,
                ..Fill::bare(bus, &snapshot, &form)
            };
            path::text(&path::fill(&Value::String((*t).to_owned()), &fill), "").unwrap_or_default()
        }
        Id::Call(step, at) => {
            let snapshot = out.clone();
            let fill = Fill {
                name,
                ..Fill::bare(bus, &snapshot, &form)
            };
            let answer = call(d, step, &fill).await?;
            merge(&mut out, answer);
            // A call that finds by a filter (an alarm by its name) answers none rather than refusing: no id is a refusal.
            path::text(&out, at).ok_or_else(|| {
                ApiError::new(
                    404,
                    "NOT_FOUND",
                    format!("There is no {} named {name}.", kind.one),
                )
            })?
        }
    };
    merge(&mut out, json!({ "$id": id }));
    Ok((id, out))
}

/// The detail's steps made in order, their answers merged into `out`; the operations made.
///
/// # Errors
/// The refusal of a step that is not optional.
pub async fn read_detail(
    bus: &Bus,
    d: &Descriptor,
    kind: &Kind,
    (id, name): (&str, &str),
    out: &mut Value,
) -> Result<Vec<String>, ApiError> {
    let mut made = Vec::new();
    let form = Map::new();
    for step in kind.detail {
        let snapshot = out.clone();
        let fill = Fill {
            id,
            name,
            ..Fill::bare(bus, &snapshot, &form)
        };
        made.push(step.op.to_owned());
        match call(d, step, &fill).await {
            Ok(answer) => merge(out, answer),
            Err(e) if step.optional => {
                tracing::debug!(op = step.op, code = %e.code, "an optional read was refused")
            }
            Err(e) => return Err(e),
        }
    }
    Ok(made)
}

/// The fields of a detail page, read out of `out`.
#[must_use]
pub fn fields(kind: &Kind, out: &Value) -> Vec<FieldValue> {
    kind.fields
        .iter()
        .map(|f| {
            let hidden = f
                .hide_when
                .is_some_and(|(p, eq)| p.is_empty() || path::text(out, p).is_some_and(|v| v == eq));
            let value = path::shown(out, f.path, f.format)
                .or_else(|| (!f.empty.is_empty()).then(|| f.empty.to_owned()));
            let link = value
                .as_ref()
                .and_then(|_| link(f.link, out, &path::text(out, f.path).unwrap_or_default()));
            FieldValue {
                label: f.label.to_owned(),
                value: if hidden { None } else { value },
                format: super::view::format(f.format),
                hidden,
                link,
            }
        })
        .collect()
}

/// The first of `to` whose condition holds in `out`, as a link to the resource named in `value`.
fn link(to: &[LinkTo], out: &Value, value: &str) -> Option<ResourceLink> {
    let to = to.iter().find(|l| {
        l.when
            .is_none_or(|(p, eq)| path::text(out, p).is_some_and(|v| v == eq))
    })?;
    Some(ResourceLink {
        service: to.service.to_owned(),
        kind: to.kind.to_owned(),
        name: path::tail(value),
    })
}

/// A list that belongs to the resource: its first page (or, for [`Step::answered`], what the detail's calls answered).
pub async fn sub(
    bus: &Bus,
    d: &Descriptor,
    sub: &Sub,
    (id, name): (&str, &str),
    out: &Value,
) -> SubList {
    let form = Map::new();
    let fill = Fill {
        id,
        name,
        ..Fill::bare(bus, out, &form)
    };
    let answer = if sub.step.is_answered() {
        Ok(out.clone())
    } else {
        call(d, &sub.step, &fill).await
    };
    let (rows, more, error) = match answer {
        Ok(answer) => {
            let items = items_of(&answer, sub.items, sub.item_as);
            let more = !sub.more.is_empty()
                && path::text(&answer, sub.more)
                    .is_some_and(|m| m == "true" || (!m.is_empty() && m != "false"));
            let rows = items.iter().map(|item| sub_row(sub, item)).collect();
            (rows, more, None)
        }
        Err(e) => (Vec::new(), false, Some(e.message)),
    };
    SubList {
        title: sub.title.to_owned(),
        operation: sub.step.op.to_owned(),
        columns: super::view::columns(sub.columns),
        rows,
        more,
        error,
        add: None,
    }
}

fn sub_row(sub: &Sub, item: &Value) -> ResourceRow {
    let cells: Vec<Option<String>> = sub
        .columns
        .iter()
        .map(|c| path::shown(item, c.path, c.format))
        .collect();
    let first = cells.first().cloned().flatten().unwrap_or_default();
    let link = sub.link.and_then(|(service, kind, at)| {
        Some(ResourceLink {
            service: service.to_owned(),
            kind: kind.to_owned(),
            name: at.read(item)?,
        })
    });
    ResourceRow {
        name: first.clone(),
        label: first.clone(),
        id: first,
        cells,
        link,
    }
}
