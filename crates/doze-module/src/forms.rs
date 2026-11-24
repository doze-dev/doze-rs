//! Action forms (protocol minor 2): the values a form sends, checked against the fields the action declares before the
//! module sees them, so every module refuses the same way and the app marks the field each refusal is about.

use doze_module_proto::msg::blocks::{Field, field};
use doze_module_proto::msg::error::{Diagnostic, Error, ErrorKind};
use doze_module_proto::msg::value::PathSeg;
use std::collections::BTreeMap;

/// What a module says when an app that does not know forms runs an action whose form has required fields.
pub const NEEDS_NEWER_APP: &str = "This action needs a newer doze app.";

/// A refusal about one field of a form: the app shows `message` next to it.
pub fn field_error(field: &str, message: impl Into<String>) -> Error {
    let message = message.into();
    Error::invalid(
        message.clone(),
        vec![Diagnostic {
            path: vec![PathSeg::Key(field.into())],
            message,
            hint: None,
        }],
    )
}

/// Checks `values` against `fields` and fills in the defaults of fields left out. With fields declared and no values
/// at all, the caller is an app that does not know forms: refused with [`NEEDS_NEWER_APP`] when any field is required
/// and has no default, otherwise run with the defaults.
pub fn check(fields: &[Field], values: &mut BTreeMap<String, String>) -> Result<(), Error> {
    if fields.is_empty() {
        return Ok(());
    }
    let unanswerable = fields
        .iter()
        .any(|f| f.required && f.default.is_none() && !values.contains_key(&f.id));
    if values.is_empty() && unanswerable {
        return Err(Error::new(ErrorKind::Unsupported, NEEDS_NEWER_APP));
    }
    if let Some(k) = values.keys().find(|k| !fields.iter().any(|f| &f.id == *k)) {
        return Err(field_error(k, format!("this action has no field {k}")));
    }
    for f in fields {
        if !values.contains_key(&f.id)
            && let Some(d) = &f.default
        {
            values.insert(f.id.clone(), d.clone());
        }
        match values.get(&f.id) {
            None if f.required => {
                return Err(field_error(&f.id, format!("{} is required", label(f))));
            }
            None => {}
            Some(v) => one(f, v)?,
        }
    }
    Ok(())
}

fn label(f: &Field) -> &str {
    if f.label.is_empty() { &f.id } else { &f.label }
}

/// One value against its field's kind.
fn one(f: &Field, v: &str) -> Result<(), Error> {
    let bad = |why: String| Err(field_error(&f.id, why));
    if f.required && v.trim().is_empty() {
        return bad(format!("{} is required", label(f)));
    }
    match f.kind.as_str() {
        field::NUMBER if !v.is_empty() => {
            let Ok(n) = v.trim().parse::<i64>() else {
                return bad(format!("{} must be a whole number", label(f)));
            };
            if let Some(min) = f.min.filter(|m| n < *m) {
                return bad(format!("{} must be at least {min}", label(f)));
            }
            if let Some(max) = f.max.filter(|m| n > *m) {
                return bad(format!("{} must be at most {max}", label(f)));
            }
            Ok(())
        }
        field::BOOL if !matches!(v, "true" | "false") => {
            bad(format!("{} must be true or false", label(f)))
        }
        field::CHOICE
            if !v.is_empty() && !f.choices.is_empty() && !f.choices.iter().any(|c| c == v) =>
        {
            bad(format!(
                "{} must be one of {}",
                label(f),
                f.choices.join(", ")
            ))
        }
        field::PAIRS => match v.lines().find(|l| !l.trim().is_empty() && !l.contains('=')) {
            Some(l) => bad(format!("{}: {l:?} is not key=value", label(f))),
            None => Ok(()),
        },
        _ => Ok(()),
    }
}

/// A `pairs` value as its keys and values, in order (`k=v` lines).
pub fn pairs(v: &str) -> Vec<(String, String)> {
    v.lines()
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim().to_string(), v.to_string()))
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn f(id: &str, kind: &str, required: bool) -> Field {
        Field {
            id: id.into(),
            label: id.into(),
            kind: kind.into(),
            required,
            ..Field::default()
        }
    }

    fn vals(kv: &[(&str, &str)]) -> BTreeMap<String, String> {
        kv.iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn field_of(e: &Error) -> String {
        match e.diagnostics[0].path.as_slice() {
            [PathSeg::Key(k)] => k.clone(),
            _ => String::new(),
        }
    }

    #[test]
    fn values_are_typed_and_defaults_filled() {
        let parts = Field {
            min: Some(1),
            max: Some(64),
            default: Some("3".into()),
            ..f("partitions", field::NUMBER, true)
        };
        let fields = [f("name", field::TEXT, true), parts];
        let mut v = vals(&[("name", "payments")]);
        check(&fields, &mut v).unwrap();
        assert_eq!(v["partitions"], "3");
        let e = check(&fields, &mut vals(&[("name", "p"), ("partitions", "0")])).unwrap_err();
        assert_eq!(field_of(&e), "partitions");
        assert!(e.message.contains("at least 1"), "{e}");
        let e = check(&fields, &mut vals(&[("name", "p"), ("partitions", "six")])).unwrap_err();
        assert!(e.message.contains("whole number"), "{e}");
        let e = check(&fields, &mut vals(&[("name", " "), ("partitions", "6")])).unwrap_err();
        assert_eq!(field_of(&e), "name");
        let e = check(&fields, &mut vals(&[("name", "p"), ("colour", "red")])).unwrap_err();
        assert_eq!(field_of(&e), "colour");
    }

    #[test]
    fn an_app_without_forms_is_told_to_update_only_when_it_must() {
        let fields = [f("name", field::TEXT, true)];
        let e = check(&fields, &mut BTreeMap::new()).unwrap_err();
        assert_eq!(
            (e.kind(), e.message.as_str()),
            (ErrorKind::Unsupported, NEEDS_NEWER_APP)
        );
        // Every field has a default: the action runs with them.
        let optional = [Field {
            default: Some("delete".into()),
            choices: vec!["delete".into(), "compact".into()],
            ..f("cleanup", field::CHOICE, true)
        }];
        let mut v = BTreeMap::new();
        check(&optional, &mut v).unwrap();
        assert_eq!(v["cleanup"], "delete");
        let e = check(&optional, &mut vals(&[("cleanup", "shred")])).unwrap_err();
        assert!(e.message.contains("one of delete, compact"), "{e}");
    }

    #[test]
    fn bools_and_pairs_are_checked() {
        let fields = [
            f("keep", field::BOOL, false),
            f("headers", field::PAIRS, false),
        ];
        check(
            &fields,
            &mut vals(&[("keep", "true"), ("headers", "a=1\nb=x=y")]),
        )
        .unwrap();
        assert!(check(&fields, &mut vals(&[("keep", "yes")])).is_err());
        assert!(check(&fields, &mut vals(&[("headers", "nope")])).is_err());
        assert_eq!(
            pairs("a=1\nb=x=y"),
            [("a".into(), "1".into()), ("b".into(), "x=y".into())]
        );
    }
}
