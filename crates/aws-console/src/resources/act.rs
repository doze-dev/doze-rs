//! The actions of a descriptor: create from a form, delete, reveal a hidden value. Each is the service's own operation, made
//! through the bus as the console.

use aws_wire::ApiError;
use serde_json::{Map, Value, json};

use super::bus::Bus;
use super::path;
use super::run::{self, Fill};
use super::{Descriptor, InputKind, Kind};
use crate::model::{Created, Deleted, Revealed};

/// A refusal of the console itself (400), before any call.
fn bad(message: String) -> ApiError {
    ApiError::new(400, "BAD_REQUEST", message)
}

/// The form's values checked against the descriptor's inputs: each required one given, each select one of its choices, each
/// number a number. Members the form does not have are dropped.
///
/// # Errors
/// What is wrong with the form, in words for the person.
pub fn checked(kind: &Kind, form: &Map<String, Value>) -> Result<Map<String, Value>, ApiError> {
    let create = kind.create.as_ref().ok_or_else(|| {
        bad(format!(
            "The console cannot create {}.",
            kind.title.to_lowercase()
        ))
    })?;
    let mut out = Map::new();
    for input in create.inputs {
        let value = match form.get(input.key) {
            Some(Value::String(s)) => s.trim().to_owned(),
            Some(Value::Bool(b)) => if *b { "true" } else { "" }.to_owned(),
            Some(Value::Number(n)) => n.to_string(),
            _ => String::new(),
        };
        if value.is_empty() {
            if input.required {
                return Err(bad(format!("{} is required.", input.label)));
            }
            continue;
        }
        match input.kind {
            InputKind::Select(choices) if !choices.contains(&value.as_str()) => {
                return Err(bad(format!(
                    "{} must be one of {}.",
                    input.label,
                    choices.join(", ")
                )));
            }
            InputKind::Number if value.parse::<i64>().is_err() => {
                return Err(bad(format!("{} must be a whole number.", input.label)));
            }
            _ => {}
        }
        out.insert(input.key.to_owned(), Value::String(value));
    }
    Ok(out)
}

/// Creates one of `kind` from `form` (already [`checked`]).
///
/// # Errors
/// The service's refusal (the name is taken, a value is out of range).
pub async fn create(
    bus: &Bus,
    (d, kind): (&Descriptor, &Kind),
    form: &Map<String, Value>,
) -> Result<Created, ApiError> {
    let Some(create) = &kind.create else {
        return Err(bad(format!(
            "The console cannot create {}.",
            kind.title.to_lowercase()
        )));
    };
    let mut out = json!({});
    let mut operations = Vec::new();
    for step in create.steps {
        let snapshot = out.clone();
        let fill = Fill::bare(bus, &snapshot, form);
        operations.push(step.op.to_owned());
        let answer = run::call(d, step, &fill).await?;
        run::merge(&mut out, answer);
    }
    let from_answer = create
        .name
        .read(&out)
        .filter(|_| !create.name.path.is_empty());
    let name = from_answer
        .or_else(|| {
            form.get(create.key)
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_default();
    Ok(Created { name, operations })
}

/// Deletes the resource named `name`.
///
/// # Errors
/// The service's refusal (a bucket that is not empty).
pub async fn delete(
    bus: &Bus,
    (d, kind): (&Descriptor, &Kind),
    name: &str,
) -> Result<Deleted, ApiError> {
    let Some(delete) = &kind.delete else {
        return Err(bad(format!(
            "The console cannot delete {}.",
            kind.title.to_lowercase()
        )));
    };
    let (id, out) = run::id_of(bus, d, kind, name).await?;
    let form = Map::new();
    let fill = Fill {
        id: &id,
        name,
        ..Fill::bare(bus, &out, &form)
    };
    run::call(d, &delete.step, &fill).await?;
    Ok(Deleted {
        name: name.to_owned(),
        operation: delete.step.op.to_owned(),
        cleared: Vec::new(),
    })
}

/// The hidden value of the resource named `name` (a secret's value): one call, which Live shows.
///
/// # Errors
/// The service's refusal.
pub async fn reveal(
    bus: &Bus,
    (d, kind): (&Descriptor, &Kind),
    name: &str,
) -> Result<Revealed, ApiError> {
    let Some(reveal) = &kind.reveal else {
        return Err(bad(format!("{} have nothing hidden.", kind.title)));
    };
    let (id, out) = run::id_of(bus, d, kind, name).await?;
    let form = Map::new();
    let fill = Fill {
        id: &id,
        name,
        ..Fill::bare(bus, &out, &form)
    };
    let answer = run::call(d, &reveal.step, &fill).await?;
    let value = reveal
        .path
        .split('|')
        .find_map(|p| path::text(&answer, p))
        .unwrap_or_default();
    Ok(Revealed {
        value,
        operation: reveal.step.op.to_owned(),
    })
}
