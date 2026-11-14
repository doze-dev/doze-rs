//! Older Smithy idioms some published models still use, rewritten to the forms the generator reads.
//!
//! - Members that target prelude shapes (`smithy.api#String`, `smithy.api#Integer`, ...) instead of a
//!   service-local shape: the prelude shape is added to the model, unconstrained.
//! - String shapes carrying the legacy `@enum` trait (Smithy 1.0): they become `enum` shapes whose members
//!   carry `@enumValue`, exactly what Smithy 2.0's own upgrade does.
//!
//! Both are no-ops for a model that uses neither (every model before `scheduler.json`).

use anyhow::{Context, Result};
use indexmap::IndexMap;
use serde_json::Value;

use crate::model::{Aggregate, MemberDef, Model, Shape, Simple, Traits};

/// The prelude simple shapes a member may target.
fn prelude_shape(id: &str) -> Option<Shape> {
    let s = Simple::default();
    Some(match id.strip_prefix("smithy.api#")? {
        "String" => Shape::String(s),
        "Boolean" | "PrimitiveBoolean" => Shape::Boolean(s),
        "Integer" | "PrimitiveInteger" => Shape::Integer(s),
        "Long" | "PrimitiveLong" => Shape::Long(s),
        "Float" | "PrimitiveFloat" => Shape::Float(s),
        "Double" | "PrimitiveDouble" => Shape::Double(s),
        "Timestamp" => Shape::Timestamp(s),
        "Blob" => Shape::Blob(s),
        "Document" => Shape::Document(s),
        _ => return None,
    })
}

/// Rewrites the legacy idioms in place.
pub fn normalize(model: &mut Model) -> Result<()> {
    let mut targets = Vec::new();
    for shape in model.shapes.values() {
        match shape {
            Shape::Structure(a) | Shape::Union(a) => {
                targets.extend(a.members.values().map(|m| m.target.clone()));
            }
            Shape::List(l) => targets.push(l.member.target.clone()),
            Shape::Map(m) => {
                targets.push(m.key.target.clone());
                targets.push(m.value.target.clone());
            }
            _ => {}
        }
    }
    for t in targets {
        if !model.shapes.contains_key(&t)
            && let Some(shape) = prelude_shape(&t)
        {
            model.shapes.insert(t, shape);
        }
    }
    for (id, shape) in &mut model.shapes {
        if let Shape::String(s) = shape
            && s.traits.contains_key("smithy.api#enum")
        {
            *shape = Shape::Enum(enum_of(&s.traits).with_context(|| id.clone())?);
        }
    }
    Ok(())
}

fn enum_of(traits: &Traits) -> Result<Aggregate> {
    let mut rest = traits.clone();
    let values = rest
        .shift_remove("smithy.api#enum")
        .and_then(|v| v.as_array().cloned())
        .context("@enum is not a list")?;
    let mut members = IndexMap::new();
    for v in values {
        let wire = v
            .get("value")
            .and_then(Value::as_str)
            .context("@enum entry without a value")?
            .to_owned();
        let name = v
            .get("name")
            .and_then(Value::as_str)
            .map_or_else(|| wire.clone(), str::to_owned);
        let mut t = Traits::new();
        t.insert("smithy.api#enumValue".to_owned(), Value::String(wire));
        members.insert(
            name,
            MemberDef {
                target: "smithy.api#Unit".to_owned(),
                traits: t,
            },
        );
    }
    Ok(Aggregate {
        members,
        traits: rest,
        mixins: Vec::new(),
    })
}
