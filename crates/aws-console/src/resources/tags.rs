//! Tags, generically: a kind names the call that reads a resource's tags and the calls that change them, and the console's
//! `GET` and `PUT …/<name>/tags` ([`crate::api::tags`]) map onto them. Each call is the service's own (`TagQueue`,
//! `TagResource`, `PutBucketTagging`), bound to the one resource the address names, and shows in Live as the console's.
//!
//! Services write tags two ways: by delta (a call that adds some, a call that removes some by key) and by replacement (S3's
//! `PutBucketTagging` takes the whole set). [`Write`] says which; a replacement reads the set, changes it, and writes it back.

use aws_wire::ApiError;
use serde_json::{Map, Value};

use super::bus::Bus;
use super::deep::{bound, id_for};
use super::run::{self, Fill};
use super::{Descriptor, Kind, Op, Step, path};
use crate::model::{Tag, TagsChange, TagsView};

/// The most tags one resource holds (AWS allows 50 on most).
pub const MOST_TAGS: usize = 50;

/// The other members a write of the whole tag set takes, worked out from what the read answered.
pub type Carry = fn(&Value) -> Map<String, Value>;

/// How a service writes its tags.
#[derive(Debug, Clone, Copy)]
pub enum Shape {
    /// `{"env": "dev"}`.
    Map,
    /// `[{"Key": "env", "Value": "dev"}]`, with the two member names (`TagKey` and `TagValue`, `key` and `value`).
    Pairs(&'static str, &'static str),
}

impl Shape {
    /// `Key` and `Value`: the commonest.
    pub const KEY_VALUE: Self = Self::Pairs("Key", "Value");

    fn read(self, value: &Value) -> Vec<Tag> {
        match (self, value) {
            (Self::Map, Value::Object(m)) => m
                .iter()
                .map(|(k, v)| Tag {
                    key: k.clone(),
                    value: v.as_str().unwrap_or_default().to_owned(),
                })
                .collect(),
            (Self::Pairs(k, v), Value::Array(items)) => items
                .iter()
                .filter_map(|i| {
                    Some(Tag {
                        key: i.get(k)?.as_str()?.to_owned(),
                        value: i
                            .get(v)
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned(),
                    })
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    fn write(self, tags: &[Tag]) -> Value {
        match self {
            Self::Map => Value::Object(
                tags.iter()
                    .map(|t| (t.key.clone(), Value::String(t.value.clone())))
                    .collect(),
            ),
            Self::Pairs(k, v) => Value::Array(
                tags.iter()
                    .map(|t| {
                        let mut m = Map::new();
                        m.insert(k.to_owned(), Value::String(t.key.clone()));
                        m.insert(v.to_owned(), Value::String(t.value.clone()));
                        Value::Object(m)
                    })
                    .collect(),
            ),
        }
    }
}

/// The calls that change a resource's tags.
#[derive(Debug, Clone, Copy)]
pub enum Write {
    /// The console reads them and does not change them.
    Read,
    /// `add` takes the tags at `at` of its input; `remove` takes the keys at `keys`.
    Delta {
        add: Op,
        at: &'static str,
        remove: Op,
        keys: &'static str,
    },
    /// `put` takes the whole set at `at`; with `clear`, an empty set is written by that call instead. `carry` works out, from
    /// what the read answered, the other members the put needs to leave the rest as it is (a stack's parameters).
    Replace {
        put: Op,
        at: &'static str,
        clear: Option<Op>,
        carry: Option<Carry>,
    },
}

/// How a kind's resources are tagged.
#[derive(Debug, Clone, Copy)]
pub struct Tags {
    /// The call that reads them; filled for the resource (`{id}`, `{name}`).
    pub list: Step,
    /// Where they are in its answer, and how they are written there.
    pub at: &'static str,
    pub shape: Shape,
    /// How the calls that change them take them, when that is not how the answer gives them (Kinesis answers a list of pairs
    /// and takes a map).
    pub sent: Option<Shape>,
    /// The error code that says a resource has no tags (S3's `NoSuchTagSet`).
    pub none: &'static str,
    pub write: Write,
}

impl Tags {
    /// Read by `list`, found at `at` in the shape `shape`, changed by `write`.
    #[must_use]
    pub const fn new(list: Step, at: &'static str, shape: Shape, write: Write) -> Self {
        Self {
            list,
            at,
            shape,
            sent: None,
            none: "",
            write,
        }
    }

    /// The same, with its changes sent in `shape`.
    #[must_use]
    pub const fn sent(mut self, shape: Shape) -> Self {
        self.sent = Some(shape);
        self
    }

    /// The same, where the error `code` means none.
    #[must_use]
    pub const fn none(mut self, code: &'static str) -> Self {
        self.none = code;
        self
    }

    fn sent_shape(&self) -> Shape {
        self.sent.unwrap_or(self.shape)
    }
}

/// A refusal of the console itself (400), before any call.
fn bad(message: impl Into<String>) -> ApiError {
    ApiError::new(400, "BAD_REQUEST", message)
}

/// What `tags` holds at `name`, read now.
///
/// # Errors
/// The service's refusal (no such resource).
pub async fn read(
    bus: &Bus,
    (d, kind): (&Descriptor, &Kind),
    tags: &Tags,
    name: &str,
) -> Result<TagsView, ApiError> {
    fetch(bus, (d, kind), tags, name)
        .await
        .map(|(view, _)| view)
}

/// The tags, and the answer they were read from.
async fn fetch(
    bus: &Bus,
    (d, kind): (&Descriptor, &Kind),
    tags: &Tags,
    name: &str,
) -> Result<(TagsView, Value), ApiError> {
    let (id, out) = run::id_of(bus, d, kind, name).await?;
    let form = Map::new();
    let fill = Fill {
        id: &id,
        name,
        ..Fill::bare(bus, &out, &form)
    };
    let answer = match run::call(d, &tags.list, &fill).await {
        Err(e) if !tags.none.is_empty() && e.code == tags.none => Value::Null,
        other => other?,
    };
    let mut held = path::get(&answer, tags.at).map_or_else(Vec::new, |v| tags.shape.read(&v));
    held.sort_by(|a, b| a.key.cmp(&b.key));
    let view = TagsView {
        tags: held,
        editable: !matches!(tags.write, Write::Read),
    };
    Ok((view, answer))
}

/// The call `op` on the resource, with `input` bound to it.
async fn make(
    bus: &Bus,
    (d, kind): (&Descriptor, &Kind),
    op: &Op,
    name: &str,
    input: Map<String, Value>,
) -> Result<(), ApiError> {
    let id = id_for(bus, (d, kind), op, name).await?;
    let body = Value::Object(bound(bus, op, (&id, name), input)?);
    bus.call(d.target, op.op, &body).await.map(drop)
}

/// Sets and removes tags of the resource named `name`; what it holds after.
///
/// # Errors
/// 400 for a kind whose tags the console only reads, an empty key, or too many tags; the service's refusal.
pub async fn change(
    bus: &Bus,
    (d, kind): (&Descriptor, &Kind),
    tags: &Tags,
    name: &str,
    change: &TagsChange,
) -> Result<TagsView, ApiError> {
    if change.set.iter().any(|t| t.key.trim().is_empty()) {
        return Err(bad("A tag needs a key."));
    }
    match &tags.write {
        Write::Read => {
            return Err(bad(format!(
                "The console only reads the tags of a {}.",
                kind.one
            )));
        }
        Write::Delta {
            add,
            at,
            remove,
            keys,
        } => {
            if !change.remove.is_empty() {
                let mut input = Map::new();
                let list = change.remove.iter().cloned().map(Value::String).collect();
                path::put_at(&mut input, keys, Value::Array(list));
                make(bus, (d, kind), remove, name, input).await?;
            }
            if !change.set.is_empty() {
                let mut input = Map::new();
                path::put_at(&mut input, at, tags.sent_shape().write(&change.set));
                make(bus, (d, kind), add, name, input).await?;
            }
        }
        Write::Replace {
            put,
            at,
            clear,
            carry,
        } => {
            let (view, answer) = fetch(bus, (d, kind), tags, name).await?;
            let mut held = view.tags;
            held.retain(|t| {
                !change.remove.contains(&t.key) && !change.set.iter().any(|s| s.key == t.key)
            });
            held.extend(change.set.iter().cloned());
            if held.len() > MOST_TAGS {
                return Err(bad(format!(
                    "A {} holds at most {MOST_TAGS} tags.",
                    kind.one
                )));
            }
            match clear.as_ref().filter(|_| held.is_empty()) {
                Some(op) => make(bus, (d, kind), op, name, Map::new()).await?,
                None => {
                    let mut input = carry.map_or_else(Map::new, |f| f(&answer));
                    path::put_at(&mut input, at, tags.sent_shape().write(&held));
                    make(bus, (d, kind), put, name, input).await?;
                }
            }
        }
    }
    read(bus, (d, kind), tags, name).await
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{Shape, Tag, path};

    fn tag(k: &str, v: &str) -> Tag {
        Tag {
            key: k.to_owned(),
            value: v.to_owned(),
        }
    }

    #[test]
    fn tags_are_read_and_written_in_both_shapes() {
        let map = json!({"env": "dev"});
        assert_eq!(Shape::Map.read(&map), [tag("env", "dev")]);
        assert_eq!(Shape::Map.write(&[tag("env", "dev")]), map);
        let pairs = json!([{"TagKey": "env", "TagValue": "dev"}]);
        let shape = Shape::Pairs("TagKey", "TagValue");
        assert_eq!(shape.read(&pairs), [tag("env", "dev")]);
        assert_eq!(shape.write(&[tag("env", "dev")]), pairs);
        assert!(Shape::KEY_VALUE.read(&json!(null)).is_empty());
    }

    #[test]
    fn a_dotted_path_makes_the_objects_on_the_way() {
        let mut m = serde_json::Map::new();
        path::put_at(&mut m, "Tagging.TagSet", json!([1]));
        path::put_at(&mut m, "Bucket", json!("b"));
        assert_eq!(
            serde_json::Value::Object(m),
            json!({"Tagging": {"TagSet": [1]}, "Bucket": "b"})
        );
    }
}
