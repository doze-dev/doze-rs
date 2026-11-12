//! Typed view of the Smithy JSON AST (`"smithy": "2.0"`).

use anyhow::{Context, Result, bail};
use indexmap::IndexMap;
use serde::Deserialize;
use serde_json::Value;

pub type Traits = IndexMap<String, Value>;

#[derive(Debug, Deserialize)]
pub struct Model {
    pub shapes: IndexMap<String, Shape>,
}

#[derive(Debug, Deserialize)]
pub struct MemberDef {
    pub target: String,
    #[serde(default)]
    pub traits: Traits,
}

#[derive(Debug, Deserialize, Default)]
pub struct Ref {
    pub target: String,
}

#[derive(Debug, Deserialize, Default)]
pub struct Simple {
    #[serde(default)]
    pub traits: Traits,
}

#[derive(Debug, Deserialize, Default)]
pub struct Aggregate {
    #[serde(default)]
    pub members: IndexMap<String, MemberDef>,
    #[serde(default)]
    pub traits: Traits,
    #[serde(default)]
    pub mixins: Vec<Ref>,
}

#[derive(Debug, Deserialize)]
pub struct ListDef {
    pub member: MemberDef,
    #[serde(default)]
    pub traits: Traits,
}

#[derive(Debug, Deserialize)]
pub struct MapDef {
    pub key: MemberDef,
    pub value: MemberDef,
    #[serde(default)]
    pub traits: Traits,
}

#[derive(Debug, Deserialize, Default)]
pub struct OperationDef {
    pub input: Option<Ref>,
    pub output: Option<Ref>,
    #[serde(default)]
    pub errors: Vec<Ref>,
    #[serde(default)]
    pub traits: Traits,
}

#[derive(Debug, Deserialize, Default)]
pub struct ServiceDef {
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub operations: Vec<Ref>,
    #[serde(default)]
    pub resources: Vec<Ref>,
    #[serde(default)]
    pub errors: Vec<Ref>,
    #[serde(default)]
    pub traits: Traits,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ResourceDef {
    #[serde(default)]
    pub operations: Vec<Ref>,
    #[serde(default)]
    pub collection_operations: Vec<Ref>,
    #[serde(default)]
    pub resources: Vec<Ref>,
    pub create: Option<Ref>,
    pub put: Option<Ref>,
    pub read: Option<Ref>,
    pub update: Option<Ref>,
    pub delete: Option<Ref>,
    pub list: Option<Ref>,
    #[serde(default)]
    pub traits: Traits,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Shape {
    Blob(Simple),
    Boolean(Simple),
    String(Simple),
    Byte(Simple),
    Short(Simple),
    Integer(Simple),
    Long(Simple),
    Float(Simple),
    Double(Simple),
    BigInteger(Simple),
    BigDecimal(Simple),
    Timestamp(Simple),
    Document(Simple),
    Enum(Aggregate),
    IntEnum(Aggregate),
    Structure(Aggregate),
    Union(Aggregate),
    List(ListDef),
    Map(MapDef),
    Service(ServiceDef),
    Operation(OperationDef),
    Resource(ResourceDef),
}

pub fn local_name(id: &str) -> &str {
    id.rsplit_once('#').map_or(id, |(_, n)| n)
}

impl Shape {
    pub fn traits(&self) -> &Traits {
        match self {
            Shape::Blob(s)
            | Shape::Boolean(s)
            | Shape::String(s)
            | Shape::Byte(s)
            | Shape::Short(s)
            | Shape::Integer(s)
            | Shape::Long(s)
            | Shape::Float(s)
            | Shape::Double(s)
            | Shape::BigInteger(s)
            | Shape::BigDecimal(s)
            | Shape::Timestamp(s)
            | Shape::Document(s) => &s.traits,
            Shape::Enum(a) | Shape::IntEnum(a) | Shape::Structure(a) | Shape::Union(a) => &a.traits,
            Shape::List(l) => &l.traits,
            Shape::Map(m) => &m.traits,
            Shape::Service(s) => &s.traits,
            Shape::Operation(o) => &o.traits,
            Shape::Resource(r) => &r.traits,
        }
    }
}

/// Accessors for the traits this generator understands.
pub trait TraitsExt {
    fn has(&self, id: &str) -> bool;
    fn get_trait(&self, id: &str) -> Option<&Value>;
    fn str_trait(&self, id: &str) -> Option<&str>;
}

impl TraitsExt for Traits {
    fn has(&self, id: &str) -> bool {
        self.contains_key(&prelude(id))
    }
    fn get_trait(&self, id: &str) -> Option<&Value> {
        self.get(&prelude(id))
    }
    fn str_trait(&self, id: &str) -> Option<&str> {
        self.get_trait(id).and_then(Value::as_str)
    }
}

/// `foo` -> `smithy.api#foo`; ids that already carry a namespace pass through.
fn prelude(id: &str) -> String {
    if id.contains('#') {
        id.to_owned()
    } else {
        format!("smithy.api#{id}")
    }
}

/// Inclusive bounds from `@length` / `@range`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Bounds<T> {
    pub min: Option<T>,
    pub max: Option<T>,
}

/// The constraint traits that make a shape worth a newtype.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Constraints {
    pub length: Option<Bounds<u64>>,
    pub range: Option<Bounds<serde_json::Number>>,
    pub pattern: Option<String>,
}

impl Constraints {
    pub fn is_empty(&self) -> bool {
        self.length.is_none() && self.range.is_none() && self.pattern.is_none()
    }

    /// Reads the shape's own constraint traits.
    pub fn from_traits(t: &Traits) -> Result<Self> {
        let length = match t.get_trait("length") {
            Some(v) => Some(Bounds {
                min: v
                    .get("min")
                    .map(|n| n.as_u64().context("length.min"))
                    .transpose()?,
                max: v
                    .get("max")
                    .map(|n| n.as_u64().context("length.max"))
                    .transpose()?,
            }),
            None => None,
        };
        let range = t.get_trait("range").map(|v| Bounds {
            min: v.get("min").and_then(|n| n.as_number().cloned()),
            max: v.get("max").and_then(|n| n.as_number().cloned()),
        });
        let pattern = match t.get_trait("pattern") {
            Some(Value::String(s)) => Some(s.clone()),
            Some(other) => bail!("pattern trait is not a string: {other}"),
            None => None,
        };
        Ok(Self {
            length,
            range,
            pattern,
        })
    }

    /// Member-level constraint traits override the target shape's, trait by trait.
    pub fn overridden_by(&self, member: &Constraints) -> Constraints {
        Constraints {
            length: member.length.clone().or_else(|| self.length.clone()),
            range: member.range.clone().or_else(|| self.range.clone()),
            pattern: member.pattern.clone().or_else(|| self.pattern.clone()),
        }
    }
}
