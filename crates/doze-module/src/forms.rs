//! Action forms (protocol minor 2): the values a form sends, checked against the fields the action declares before the
//! module sees them, so every module refuses the same way and the app marks the field each refusal is about.

use doze_module_proto::msg::blocks::{Field, field};
use doze_module_proto::msg::error::{Error, ErrorKind};
use std::collections::BTreeMap;

/// What a module says when an app that does not know forms runs an action whose form has required fields.
pub const NEEDS_NEWER_APP: &str = "This action needs a newer doze app.";

/// A refusal about one field of a form: the app shows `message` next to it, and the error says it once.
pub fn field_error(field: &str, message: impl Into<String>) -> Error {
    Error::for_field(field, message)
}

/// Checks `values` against `fields` and fills in the defaults of fields left out; a number is kept trimmed. `form`
/// says the caller knows forms (`RunAction::form`): without it and without values, the caller is a host or app that
/// does not know forms, refused with [`NEEDS_NEWER_APP`] when a field is required and has no default, otherwise run
/// with the defaults. With it, a required field left blank is refused by name.
pub fn check(
    fields: &[Field],
    form: bool,
    values: &mut BTreeMap<String, String>,
) -> Result<(), Error> {
    if fields.is_empty() {
        return Ok(());
    }
    let unanswerable = fields
        .iter()
        .any(|f| f.required && f.default.is_none() && !values.contains_key(&f.id));
    if !form && values.is_empty() && unanswerable {
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
        if f.kind == field::NUMBER
            && let Some(v) = values.get_mut(&f.id)
        {
            *v = v.trim().to_string();
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
            let Ok(n) = v.parse::<i64>() else {
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
        field::PAIRS => {
            let line = |l: &&str| match l.split_once('=') {
                _ if l.trim().is_empty() => false,
                Some((k, _)) => k.trim().is_empty(),
                None => true,
            };
            match v.lines().find(line) {
                Some(l) if l.contains('=') => bad(format!("{}: {l:?} has no key", label(f))),
                Some(l) => bad(format!("{}: {l:?} is not key=value", label(f))),
                None => Ok(()),
            }
        }
        _ => Ok(()),
    }
}

/// An older app's one box, put where the form would have taken it: an app that does not know forms (`form` false, no
/// values) sends what was typed in the action's one box as `input`. When one field is clearly the box's (the only
/// required field without a default, else the only required field), it gets the input, so a form with a required
/// field still runs from an older app. With two candidates nothing is filled, and [`check`] says the action needs a
/// newer app.
pub fn one_box(
    fields: &[Field],
    form: bool,
    input: Option<&str>,
    values: &mut BTreeMap<String, String>,
) {
    let Some(input) = input.filter(|i| !i.trim().is_empty()) else {
        return;
    };
    if form || !values.is_empty() {
        return;
    }
    let sole = |want: &dyn Fn(&Field) -> bool| {
        let mut it = fields.iter().filter(|f| want(f));
        match (it.next(), it.next()) {
            (Some(f), None) => Some(f),
            _ => None,
        }
    };
    let field = sole(&|f| f.required && f.default.is_none()).or_else(|| {
        fields
            .iter()
            .all(|f| !f.required || f.default.is_some())
            .then(|| sole(&|f| f.required))
            .flatten()
    });
    if let Some(f) = field {
        values.insert(f.id.clone(), input.to_string());
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

    #[test]
    fn an_older_apps_one_box_fills_the_one_required_field() {
        let name = || Field {
            default: None,
            ..f("name", field::TEXT, true)
        };
        let size = || Field {
            default: Some("1".into()),
            ..f("size", field::NUMBER, true)
        };
        let mut v = BTreeMap::new();
        one_box(&[name(), size()], false, Some("orders"), &mut v);
        check(&[name(), size()], false, &mut v).unwrap();
        assert_eq!((v["name"].as_str(), v["size"].as_str()), ("orders", "1"));
        // Two required fields without defaults: whose box was it? Nothing is guessed; the app is told to update.
        let mut v = BTreeMap::new();
        let two = [name(), f("value", field::TEXT, true)];
        one_box(&two, false, Some("k v"), &mut v);
        assert!(v.is_empty());
        let e = check(&two, false, &mut v).unwrap_err();
        assert_eq!(e.message, NEEDS_NEWER_APP);
        // A form, or values already sent, or nothing typed: left alone.
        for (form, input) in [(true, Some("x")), (false, Some("  ")), (false, None)] {
            let mut v = BTreeMap::new();
            one_box(&[name()], form, input, &mut v);
            assert!(v.is_empty(), "{form} {input:?}");
        }
    }

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
        assert!(e.diagnostics.is_empty(), "said once: {}", e.describe());
        e.field.clone().unwrap_or_default()
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
        check(&fields, true, &mut v).unwrap();
        assert_eq!(v["partitions"], "3");
        let mut v = vals(&[("name", "payments"), ("partitions", " 6 ")]);
        check(&fields, true, &mut v).unwrap();
        assert_eq!(
            v["partitions"], "6",
            "the number checked is the number passed on"
        );
        let e = check(
            &fields,
            true,
            &mut vals(&[("name", "p"), ("partitions", "0")]),
        )
        .unwrap_err();
        assert_eq!(field_of(&e), "partitions");
        assert!(e.message.contains("at least 1"), "{e}");
        let e = check(
            &fields,
            true,
            &mut vals(&[("name", "p"), ("partitions", "six")]),
        )
        .unwrap_err();
        assert!(e.message.contains("whole number"), "{e}");
        let e = check(
            &fields,
            true,
            &mut vals(&[("name", " "), ("partitions", "6")]),
        )
        .unwrap_err();
        assert_eq!(field_of(&e), "name");
        let e = check(
            &fields,
            true,
            &mut vals(&[("name", "p"), ("colour", "red")]),
        )
        .unwrap_err();
        assert_eq!(field_of(&e), "colour");
    }

    #[test]
    fn an_app_without_forms_is_told_to_update_only_when_it_must() {
        let fields = [f("name", field::TEXT, true)];
        let e = check(&fields, false, &mut BTreeMap::new()).unwrap_err();
        assert_eq!(
            (e.kind(), e.message.as_str()),
            (ErrorKind::Unsupported, NEEDS_NEWER_APP)
        );
        // A new app whose form was left blank (no values on the wire) is told which field, not to update.
        let e = check(&fields, true, &mut BTreeMap::new()).unwrap_err();
        assert_eq!(
            (field_of(&e), e.message.as_str()),
            ("name".into(), "name is required")
        );
        // Every field has a default: the action runs with them.
        let optional = [Field {
            default: Some("delete".into()),
            choices: vec!["delete".into(), "compact".into()],
            ..f("cleanup", field::CHOICE, true)
        }];
        let mut v = BTreeMap::new();
        check(&optional, false, &mut v).unwrap();
        assert_eq!(v["cleanup"], "delete");
        let e = check(&optional, false, &mut vals(&[("cleanup", "shred")])).unwrap_err();
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
            true,
            &mut vals(&[("keep", "true"), ("headers", "a=1\nb=x=y")]),
        )
        .unwrap();
        let e = check(&fields, true, &mut vals(&[("headers", "a=1\n =v")])).unwrap_err();
        assert_eq!(
            (field_of(&e), e.message.as_str()),
            ("headers".into(), "headers: \" =v\" has no key")
        );
        assert!(check(&fields, true, &mut vals(&[("keep", "yes")])).is_err());
        assert!(check(&fields, true, &mut vals(&[("headers", "nope")])).is_err());
        assert_eq!(
            pairs("a=1\nb=x=y"),
            [("a".into(), "1".into()), ("b".into(), "x=y".into())]
        );
    }
}
