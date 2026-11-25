//! An action's form as a person fills it in (module protocol minor 2): each typed field with what it holds, its
//! defaults filled in, a field's error under it. What the app checks before sending is only what the field says of
//! itself (required, a whole number in its range), in the SDK's words, so a person learns it before a round trip;
//! the module checks again and has the last word. Every field is sent, blanks included, with `form` set.

use super::console::Action;
use doze_control::FieldView;
use std::collections::BTreeMap;

/// The field kinds a form draws; another is a text box.
pub mod kind {
    pub const TEXT: &str = "text";
    pub const SECRET: &str = "secret";
    pub const NUMBER: &str = "number";
    pub const CHOICE: &str = "choice";
    pub const BOOL: &str = "bool";
    pub const PAIRS: &str = "pairs";
}

/// A form open over the page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Form {
    pub instance: String,
    pub action: Action,
    /// What each field holds, in the order of `action.fields`.
    pub values: Vec<String>,
    /// The field marked, by id, and why; or the form's own refusal (`field` empty).
    pub error: Option<(String, String)>,
    /// Sent, waiting for the module.
    pub running: bool,
}

impl Form {
    pub fn new(instance: &str, action: &Action) -> Form {
        let values = action
            .fields
            .iter()
            .map(|f| match (f.kind.as_str(), f.default.as_str()) {
                (kind::BOOL, "") => "false".to_owned(),
                (kind::CHOICE, "") if f.required => f.choices.first().cloned().unwrap_or_default(),
                (_, d) => d.to_owned(),
            })
            .collect();
        Form {
            instance: instance.to_owned(),
            action: action.clone(),
            values,
            error: None,
            running: false,
        }
    }

    pub fn fields(&self) -> &[FieldView] {
        &self.action.fields
    }

    /// What field `i` holds.
    pub fn value(&self, i: usize) -> &str {
        self.values.get(i).map_or("", String::as_str)
    }

    /// Field `i` changed: its error goes with what was wrong.
    pub fn set(&mut self, i: usize, v: &str) {
        if let Some(slot) = self.values.get_mut(i) {
            v.clone_into(slot);
        }
        let id = self.action.fields.get(i).map(|f| f.id.as_str());
        if self
            .error
            .as_ref()
            .is_some_and(|(f, _)| Some(f.as_str()) == id)
        {
            self.error = None;
        }
    }

    /// Flips a bool field.
    pub fn flip(&mut self, i: usize) {
        let on = self.value(i) == "true";
        self.set(i, if on { "false" } else { "true" });
    }

    /// The error under field `id`, if it is the one marked.
    pub fn error_of(&self, id: &str) -> Option<&str> {
        self.error
            .as_ref()
            .filter(|(f, _)| f == id)
            .map(|(_, m)| m.as_str())
    }

    /// The form's own error, not about one field (or about a field it does not have).
    pub fn general_error(&self) -> Option<&str> {
        self.error
            .as_ref()
            .filter(|(f, _)| f.is_empty() || !self.action.fields.iter().any(|x| &x.id == f))
            .map(|(_, m)| m.as_str())
    }

    /// The values to send, every field's (blanks too), once each field holds what it says it takes; else the first
    /// field that does not, marked.
    pub fn submit(&mut self) -> Option<BTreeMap<String, String>> {
        for (f, v) in self.action.fields.iter().zip(&self.values) {
            if let Some(why) = wrong(f, v) {
                self.error = Some((f.id.clone(), why));
                return None;
            }
        }
        self.error = None;
        Some(
            self.action
                .fields
                .iter()
                .zip(&self.values)
                .map(|(f, v)| {
                    let v = if f.kind == kind::NUMBER {
                        v.trim()
                    } else {
                        v.as_str()
                    };
                    (f.id.clone(), v.to_owned())
                })
                .collect(),
        )
    }

    /// The module refused: its words, under the field it named.
    pub fn refused(&mut self, message: &str, field: &str) {
        self.running = false;
        let label = self.action.fields.iter().find(|f| f.id == field).map(label);
        // The module's words are about the field; under it, they need not name it again ("name: already exists").
        let message = match label {
            Some(l) => message
                .strip_prefix(&format!("{l}: "))
                .unwrap_or(message)
                .to_owned(),
            None => message.to_owned(),
        };
        self.error = Some((field.to_owned(), sentence(&message)));
    }
}

fn label(f: &FieldView) -> &str {
    if f.label.is_empty() { &f.id } else { &f.label }
}

/// What is wrong with `v` for `f`, in the SDK's words.
fn wrong(f: &FieldView, v: &str) -> Option<String> {
    if f.required && v.trim().is_empty() {
        return Some(format!("{} is required.", label(f)));
    }
    if f.kind == kind::NUMBER && !v.trim().is_empty() {
        let Ok(n) = v.trim().parse::<i64>() else {
            return Some(format!("{} must be a whole number.", label(f)));
        };
        if let Some(min) = f.min.filter(|m| n < *m) {
            return Some(format!("{} must be at least {min}.", label(f)));
        }
        if let Some(max) = f.max.filter(|m| n > *m) {
            return Some(format!("{} must be at most {max}.", label(f)));
        }
    }
    if f.kind == kind::PAIRS
        && let Some(l) = v.lines().find(|l| {
            !l.trim().is_empty() && l.split_once('=').is_none_or(|(k, _)| k.trim().is_empty())
        })
    {
        return Some(format!("“{}” is not key=value.", l.trim()));
    }
    None
}

/// Words as a sentence: a capital first and a full stop.
fn sentence(s: &str) -> String {
    let s = s.trim();
    let mut c = s.chars();
    let mut out: String = c
        .next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default();
    if !out.is_empty() && !out.ends_with(['.', '!', '?']) {
        out.push('.');
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn field(id: &str, kind: &str, required: bool) -> FieldView {
        FieldView {
            id: id.into(),
            label: id.to_uppercase(),
            kind: kind.into(),
            required,
            ..FieldView::default()
        }
    }

    fn form() -> Form {
        let a = Action {
            id: "create".into(),
            label: "Create".into(),
            key: 'C',
            destructive: false,
            hint: String::new(),
            resource: String::new(),
            row: String::new(),
            fields: vec![
                field("name", kind::TEXT, true),
                FieldView {
                    default: "1".into(),
                    min: Some(1),
                    max: Some(10),
                    ..field("size", kind::NUMBER, true)
                },
                field("keep", kind::BOOL, false),
                FieldView {
                    choices: vec!["delete".into(), "compact".into()],
                    ..field("cleanup", kind::CHOICE, true)
                },
                field("headers", kind::PAIRS, false),
                field("token", kind::SECRET, false),
            ],
        };
        Form::new("db", &a)
    }

    #[test]
    fn defaults_are_filled_and_every_field_is_sent() {
        let mut f = form();
        assert_eq!(f.values, ["", "1", "false", "delete", "", ""]);
        assert_eq!(f.submit(), None);
        assert_eq!(f.error_of("name"), Some("NAME is required."));
        f.set(0, "orders");
        assert_eq!(f.error, None, "typing in the marked field clears it");
        f.set(1, " 4 ");
        f.flip(2);
        let sent = f.submit().unwrap();
        assert_eq!(sent.len(), 6, "blanks included");
        assert_eq!(sent["size"], "4");
        assert_eq!(sent["keep"], "true");
        assert_eq!(sent["token"], "");
    }

    #[test]
    fn numbers_and_pairs_are_checked_before_sending() {
        let mut f = form();
        f.set(0, "x");
        f.set(1, "11");
        assert_eq!(f.submit(), None);
        assert_eq!(f.error_of("size"), Some("SIZE must be at most 10."));
        f.set(1, "2.5");
        f.submit();
        assert_eq!(f.error_of("size"), Some("SIZE must be a whole number."));
        f.set(1, "2");
        f.set(4, "a=1\nnope");
        f.submit();
        assert_eq!(f.error_of("headers"), Some("“nope” is not key=value."));
    }

    #[test]
    fn the_modules_refusal_goes_under_its_field() {
        let mut f = form();
        f.refused("NAME: already exists", "name");
        assert_eq!(f.error_of("name"), Some("Already exists."));
        f.refused("the broker said no", "");
        assert_eq!(f.general_error(), Some("The broker said no."));
    }
}
