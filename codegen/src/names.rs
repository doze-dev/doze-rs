//! Type names for Smithy shapes.

use heck::ToUpperCamelCase;

/// Names that would shadow something the generated code relies on: the marker types and traits of
/// `aws_schema::view`, which `keys.rs` imports, and the `Operation` enum.
const RESERVED_TYPES: &[&str] = &[
    "Bool",
    "Int",
    "Float",
    "Str",
    "Blob",
    "Time",
    "Other",
    "Obj",
    "List",
    "Map",
    "Key",
    "Shape",
    "Operation",
    "Option",
    "Result",
    "String",
    "Vec",
    "Box",
    "Some",
    "None",
    "Ok",
    "Err",
    "Self",
    "Default",
    "Clone",
    "Copy",
    "Send",
    "Sync",
    "Sized",
    "Drop",
    "Eq",
    "Ord",
    "PartialEq",
    "PartialOrd",
    "From",
    "Into",
    "TryFrom",
    "TryInto",
    "AsRef",
    "ToString",
    "Iterator",
    "IntoIterator",
    "Debug",
    "Display",
    "Hash",
    "FromStr",
    "Formatter",
    "Unit",
];

/// A type name for a Smithy shape name. Names that are already UpperCamelCase are kept verbatim so
/// they stay greppable against the model (`DBInstance` stays `DBInstance`).
pub fn type_name(shape: &str) -> String {
    let mut s = if shape.contains('_') {
        shape.to_upper_camel_case()
    } else {
        let mut cs = shape.chars();
        match cs.next() {
            Some(first) => first.to_ascii_uppercase().to_string() + cs.as_str(),
            None => String::new(),
        }
    };
    if s.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        s.insert(0, 'T');
    }
    if RESERVED_TYPES.contains(&s.as_str()) {
        s.push_str("Shape");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_names_are_kept_and_collisions_are_suffixed() {
        assert_eq!(type_name("DBInstance"), "DBInstance");
        assert_eq!(type_name("queue_url"), "QueueUrl");
        assert_eq!(type_name("List"), "ListShape");
        assert_eq!(type_name("Shape"), "ShapeShape");
        assert_eq!(type_name("2fa"), "T2fa");
    }
}
