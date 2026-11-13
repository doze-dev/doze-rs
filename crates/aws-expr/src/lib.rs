//! DynamoDB's data model and expression languages, as a library with no I/O: numbers (exact decimals of up to 38 digits),
//! attribute values and items with their JSON wire form and sizes, and the five expression languages (condition and filter,
//! key condition, projection, update) with AWS's error wording.
//!
//! The service crate resolves a request's `ExpressionAttributeNames` and `ExpressionAttributeValues` into a [`Scope`], parses
//! each expression against it, checks that every name and value was used ([`Scope::finish`]), and then evaluates.

mod ast;
mod cond;
mod keycond;
mod lex;
mod number;
mod parse;
mod parse_cond;
mod parse_update;
mod path;
mod reserved;
mod scope;
mod update;
mod value;

pub use ast::{Cmp, Cond, Elem, Func, NameMap, Operand, Path, SetValue, Update, ValueMap};
pub use keycond::{KeyCondition, Range, parse_key_condition};
pub use number::{MAX_DIGITS, Number, NumberError};
pub use parse_cond::parse_condition;
pub use parse_update::{parse_projection, parse_update};
pub use path::{get as get_path, project};
pub use reserved::is_reserved;
pub use scope::{Kind, Scope};
pub use value::{Item, MAX_DEPTH, Value, item_from_json, item_size, item_to_json};

/// A refusal in AWS's words: always a `ValidationException`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invalid(pub String);

impl std::fmt::Display for Invalid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Invalid {}

/// Whether `text` is accepted as each kind of expression, with no names or values defined. Exists so the parsers can be fuzzed.
#[doc(hidden)]
pub fn parse_all(text: &str) -> usize {
    let (names, values) = (NameMap::new(), ValueMap::new());
    let mut accepted = 0;
    for kind in [Kind::Condition, Kind::Filter] {
        accepted +=
            usize::from(parse_condition(text, kind, &mut Scope::new(&names, &values)).is_ok());
    }
    accepted += usize::from(parse_update(text, &mut Scope::new(&names, &values)).is_ok());
    accepted += usize::from(parse_projection(text, &mut Scope::new(&names, &values)).is_ok());
    accepted += usize::from(
        parse_key_condition(
            text,
            &mut Scope::new(&names, &values),
            ("pk", "S"),
            Some(("sk", "N")),
        )
        .is_ok(),
    );
    accepted += usize::from(Number::parse(text).is_ok());
    if let Ok(json) = serde_json::from_str(text) {
        accepted += usize::from(Value::from_json(&json).is_ok());
    }
    accepted
}
