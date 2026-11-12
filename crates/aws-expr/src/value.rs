//! DynamoDB's attribute values and items, their JSON wire form and the size AWS counts for them.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value as Json};

use crate::Invalid;
use crate::number::Number;

/// Deepest nesting of lists and maps in an item. `(memory)`: the developer guide says 32 levels.
pub const MAX_DEPTH: usize = 32;

/// An item: attribute names to values.
pub type Item = BTreeMap<String, Value>;

/// One attribute value.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Value {
    /// A string.
    S(String),
    /// A number.
    N(Number),
    /// Bytes.
    B(Vec<u8>),
    /// A boolean.
    Bool(bool),
    /// Null.
    Null,
    /// A set of strings.
    Ss(BTreeSet<String>),
    /// A set of numbers.
    Ns(BTreeSet<Number>),
    /// A set of byte strings.
    Bs(BTreeSet<Vec<u8>>),
    /// A list.
    L(Vec<Value>),
    /// A map.
    M(BTreeMap<String, Value>),
}

/// An error that is a `ValidationException` in this wording.
fn invalid(text: impl Into<String>) -> Invalid {
    Invalid(text.into())
}

fn bad_param(text: &str) -> Invalid {
    invalid(format!("One or more parameter values were invalid: {text}"))
}

impl Value {
    /// The type's wire name: `S`, `N`, `B`, `SS`, `NS`, `BS`, `BOOL`, `NULL`, `L`, `M`.
    #[must_use]
    pub const fn type_name(&self) -> &'static str {
        match self {
            Self::S(_) => "S",
            Self::N(_) => "N",
            Self::B(_) => "B",
            Self::Bool(_) => "BOOL",
            Self::Null => "NULL",
            Self::Ss(_) => "SS",
            Self::Ns(_) => "NS",
            Self::Bs(_) => "BS",
            Self::L(_) => "L",
            Self::M(_) => "M",
        }
    }

    /// Reads `{"S": "x"}` and the like.
    ///
    /// # Errors
    /// AWS's refusal of a malformed value.
    pub fn from_json(json: &Json) -> Result<Self, Invalid> {
        Self::read(json, 1)
    }

    fn read(json: &Json, depth: usize) -> Result<Self, Invalid> {
        if depth > MAX_DEPTH + 1 {
            return Err(bad_param("Nesting Levels have exceeded supported limits"));
        }
        let Json::Object(obj) = json else {
            return Err(invalid(
                "Supplied AttributeValue is empty, must contain exactly one of the supported datatypes",
            ));
        };
        let mut entries = obj.iter();
        let (Some((tag, body)), None) = (entries.next(), entries.next()) else {
            return Err(invalid(if obj.is_empty() {
                "Supplied AttributeValue is empty, must contain exactly one of the supported datatypes"
            } else {
                "Supplied AttributeValue has more than one datatypes set, must contain exactly one of the supported datatypes"
            }));
        };
        match (tag.as_str(), body) {
            ("S", Json::String(s)) => Ok(Self::S(s.clone())),
            ("N", Json::String(s)) => Number::parse(s)
                .map(Self::N)
                .map_err(|e| invalid(e.message(s))),
            ("B", Json::String(s)) => aws_core::base64::decode(s)
                .map(Self::B)
                .ok_or_else(|| invalid("The provided value is not valid base64")),
            ("BOOL", Json::Bool(b)) => Ok(Self::Bool(*b)),
            ("NULL", Json::Bool(_)) => Ok(Self::Null),
            ("SS", Json::Array(a)) => strings(a)
                .and_then(|v| set(v, "string", "SS"))
                .map(Self::Ss),
            ("NS", Json::Array(a)) => numbers(a)
                .and_then(|v| set(v, "number", "NS"))
                .map(Self::Ns),
            ("BS", Json::Array(a)) => blobs(a).and_then(|v| set(v, "binary", "BS")).map(Self::Bs),
            ("L", Json::Array(a)) => a
                .iter()
                .map(|e| Self::read(e, depth + 1))
                .collect::<Result<_, _>>()
                .map(Self::L),
            ("M", Json::Object(m)) => m
                .iter()
                .map(|(k, v)| Self::read(v, depth + 1).map(|v| (k.clone(), v)))
                .collect::<Result<_, _>>()
                .map(Self::M),
            _ => Err(invalid(
                "Supplied AttributeValue is empty, must contain exactly one of the supported datatypes",
            )),
        }
    }

    /// The wire form.
    #[must_use]
    pub fn to_json(&self) -> Json {
        let (tag, body) = match self {
            Self::S(s) => ("S", Json::String(s.clone())),
            Self::N(n) => ("N", Json::String(n.to_string())),
            Self::B(b) => ("B", Json::String(aws_core::base64::encode(b))),
            Self::Bool(b) => ("BOOL", Json::Bool(*b)),
            Self::Null => ("NULL", Json::Bool(true)),
            Self::Ss(s) => ("SS", s.iter().map(|x| Json::String(x.clone())).collect()),
            Self::Ns(s) => (
                "NS",
                s.iter().map(|x| Json::String(x.to_string())).collect(),
            ),
            Self::Bs(s) => (
                "BS",
                s.iter()
                    .map(|x| Json::String(aws_core::base64::encode(x)))
                    .collect(),
            ),
            Self::L(l) => ("L", l.iter().map(Self::to_json).collect()),
            Self::M(m) => (
                "M",
                Json::Object(m.iter().map(|(k, v)| (k.clone(), v.to_json())).collect()),
            ),
        };
        let mut out = Map::new();
        out.insert(tag.to_owned(), body);
        Json::Object(out)
    }

    /// The bytes AWS counts for the value (not its name). Strings and binary count their length, a number about a byte per two
    /// digits, a boolean or null one byte, a list or map three plus one per element plus its parts.
    #[must_use]
    pub fn size(&self) -> usize {
        match self {
            Self::S(s) => s.len(),
            Self::N(n) => n.size(),
            Self::B(b) => b.len(),
            Self::Bool(_) | Self::Null => 1,
            Self::Ss(s) => s.iter().map(String::len).sum(),
            Self::Ns(s) => s.iter().map(Number::size).sum(),
            Self::Bs(s) => s.iter().map(Vec::len).sum(),
            Self::L(l) => 3 + l.iter().map(|v| 1 + v.size()).sum::<usize>(),
            Self::M(m) => 3 + m.iter().map(|(k, v)| 1 + k.len() + v.size()).sum::<usize>(),
        }
    }

    /// How deeply lists and maps nest inside this value (a scalar is 0).
    #[must_use]
    pub fn depth(&self) -> usize {
        match self {
            Self::L(l) => 1 + l.iter().map(Self::depth).max().unwrap_or(0),
            Self::M(m) => 1 + m.values().map(Self::depth).max().unwrap_or(0),
            _ => 0,
        }
    }

    /// Orders two values of the same scalar type (string, number, binary); `None` for any other pair.
    #[must_use]
    pub fn compare(&self, other: &Self) -> Option<Ordering> {
        match (self, other) {
            (Self::S(a), Self::S(b)) => Some(a.as_bytes().cmp(b.as_bytes())),
            (Self::N(a), Self::N(b)) => Some(a.cmp(b)),
            (Self::B(a), Self::B(b)) => Some(a.cmp(b)),
            _ => None,
        }
    }

    /// Whether the value is a set of any kind.
    #[must_use]
    pub const fn is_set(&self) -> bool {
        matches!(self, Self::Ss(_) | Self::Ns(_) | Self::Bs(_))
    }
}

/// An element of a set as a message prints it.
trait Shown {
    fn shown(&self) -> String;
}

impl Shown for String {
    fn shown(&self) -> String {
        self.clone()
    }
}

impl Shown for Number {
    fn shown(&self) -> String {
        self.to_string()
    }
}

impl Shown for Vec<u8> {
    fn shown(&self) -> String {
        aws_core::base64::encode(self)
    }
}

fn strings(a: &[Json]) -> Result<Vec<String>, Invalid> {
    a.iter()
        .map(|e| match e {
            Json::String(s) => Ok(s.clone()),
            _ => Err(invalid("Supplied AttributeValue is empty, must contain exactly one of the supported datatypes")),
        })
        .collect()
}

fn numbers(a: &[Json]) -> Result<Vec<Number>, Invalid> {
    strings(a)?
        .iter()
        .map(|s| Number::parse(s).map_err(|e| invalid(e.message(s))))
        .collect()
}

fn blobs(a: &[Json]) -> Result<Vec<Vec<u8>>, Invalid> {
    strings(a)?
        .iter()
        .map(|s| {
            aws_core::base64::decode(s)
                .ok_or_else(|| invalid("The provided value is not valid base64"))
        })
        .collect()
}

/// A set from its elements, refusing an empty one or one with a repeat. The empty-set text is recorded for string sets (with its
/// double space and its count prefix); the other kinds and the repeat text are `(memory)`.
fn set<T: Ord + Shown>(items: Vec<T>, noun: &str, kind: &str) -> Result<BTreeSet<T>, Invalid> {
    if items.is_empty() {
        return Err(invalid(format!(
            "1 validation error detected: One or more parameter values were invalid: An {noun} set  may not be empty"
        )));
    }
    let count = items.len();
    let shown = items
        .iter()
        .map(Shown::shown)
        .collect::<Vec<_>>()
        .join(", ");
    let set: BTreeSet<T> = items.into_iter().collect();
    if set.len() != count {
        return Err(bad_param(&format!(
            "Input collection [{shown}] of type {kind} contains duplicates."
        )));
    }
    Ok(set)
}

/// Reads an item from `{"a": {"S": "x"}, ...}`.
///
/// # Errors
/// AWS's refusal of a malformed attribute value.
pub fn item_from_json(json: &Json) -> Result<Item, Invalid> {
    let Json::Object(obj) = json else {
        return Err(invalid(
            "Supplied AttributeValue is empty, must contain exactly one of the supported datatypes",
        ));
    };
    obj.iter()
        .map(|(k, v)| Value::from_json(v).map(|v| (k.clone(), v)))
        .collect()
}

/// The wire form of an item.
#[must_use]
pub fn item_to_json(item: &Item) -> Json {
    Json::Object(item.iter().map(|(k, v)| (k.clone(), v.to_json())).collect())
}

/// The size AWS counts for an item: every attribute's name and value.
#[must_use]
pub fn item_size(item: &Item) -> usize {
    item.iter().map(|(k, v)| k.len() + v.size()).sum()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn every_type_round_trips() {
        let wire = json!({
            "s": {"S": "a"}, "n": {"N": "1.50"}, "b": {"B": "AP8="}, "t": {"BOOL": true}, "z": {"NULL": true},
            "ss": {"SS": ["b", "a"]}, "ns": {"NS": ["2", "1"]}, "bs": {"BS": ["AP8="]},
            "l": {"L": [{"S": "x"}, {"N": "2"}]}, "m": {"M": {"i": {"S": "y"}}}
        });
        let item = item_from_json(&wire).unwrap();
        let back = item_to_json(&item);
        assert_eq!(back["n"], json!({"N": "1.5"}));
        assert_eq!(back["ss"], json!({"SS": ["a", "b"]}));
        assert_eq!(back["ns"], json!({"NS": ["1", "2"]}));
        assert_eq!(back["b"], wire["b"]);
    }

    #[test]
    fn malformed_values_are_refused_in_aws_words() {
        let err = |j: Json| Value::from_json(&j).unwrap_err().0;
        assert!(err(json!({})).contains("is empty"));
        assert!(err(json!({"S": "a", "N": "1"})).contains("more than one datatypes"));
        assert_eq!(
            err(json!({"SS": []})),
            "1 validation error detected: One or more parameter values were invalid: An string set  may not be empty"
        );
        assert!(err(json!({"SS": ["a", "a"]})).contains("contains duplicates"));
        assert!(err(json!({"N": "abc"})).contains("cannot be converted to a numeric value: abc"));
    }

    #[test]
    fn sizes_follow_the_documented_rules() {
        let item = item_from_json(
            &json!({"ab": {"S": "xyz"}, "n": {"N": "12"}, "l": {"L": [{"BOOL": true}]}}),
        )
        .unwrap();
        // ab(2)+3, n(1)+2, l(1)+(3+1+1)
        assert_eq!(item_size(&item), 5 + 3 + 6);
    }

    #[test]
    fn depth_is_counted() {
        let mut v = json!({"S": "x"});
        for _ in 0..40 {
            v = json!({"L": [v]});
        }
        assert!(Value::from_json(&v).is_err());
        assert_eq!(
            Value::from_json(&json!({"L": [{"L": []}]}))
                .unwrap()
                .depth(),
            2
        );
    }
}
