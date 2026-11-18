//! Reading the members of one JSON object of a definition, reporting a member of the wrong kind where it is found. The wording of
//! these reports is from memory (`(memory)`): only `These fields are required: [StartAt]` and the `Type` value list are recorded.

use serde_json::{Map, Value};

use super::{Diagnostic, Field, Num};

pub(super) const SCHEMA: &str = "SCHEMA_VALIDATION_FAILED";

/// One object being read, where it is, and the problems found so far.
pub(super) struct Obj<'a> {
    pub(super) map: &'a Map<String, Value>,
    pub(super) at: String,
}

impl<'a> Obj<'a> {
    pub(super) fn new(map: &'a Map<String, Value>, at: String) -> Self {
        Self { map, at }
    }

    pub(super) fn here(&self, name: &str) -> String {
        format!("{}/{name}", self.at)
    }

    pub(super) fn get(&self, name: &str) -> Option<&'a Value> {
        self.map.get(name)
    }

    fn wrong(&self, name: &str, kind: &str, out: &mut Vec<Diagnostic>) {
        // (memory)
        out.push(Diagnostic::error(
            SCHEMA,
            format!("The value for the field '{name}' must be {kind}"),
            &self.here(name),
        ));
    }

    pub(super) fn text(&self, name: &str, out: &mut Vec<Diagnostic>) -> Option<String> {
        match self.get(name)? {
            Value::String(s) => Some(s.clone()),
            _ => {
                self.wrong(name, "a STRING", out);
                None
            }
        }
    }

    pub(super) fn flag(&self, name: &str, out: &mut Vec<Diagnostic>) -> bool {
        match self.get(name) {
            None => false,
            Some(Value::Bool(b)) => *b,
            Some(_) => {
                self.wrong(name, "a BOOLEAN", out);
                false
            }
        }
    }

    pub(super) fn number(&self, name: &str, out: &mut Vec<Diagnostic>) -> Option<f64> {
        match self.get(name)? {
            Value::Number(n) => n.as_f64(),
            _ => {
                self.wrong(name, "a NUMBER", out);
                None
            }
        }
    }

    pub(super) fn object(
        &self,
        name: &str,
        out: &mut Vec<Diagnostic>,
    ) -> Option<&'a Map<String, Value>> {
        match self.get(name)? {
            Value::Object(o) => Some(o),
            _ => {
                self.wrong(name, "an OBJECT", out);
                None
            }
        }
    }

    pub(super) fn array(&self, name: &str, out: &mut Vec<Diagnostic>) -> Option<&'a Vec<Value>> {
        match self.get(name)? {
            Value::Array(a) => Some(a),
            _ => {
                self.wrong(name, "an ARRAY", out);
                None
            }
        }
    }

    /// A path field: absent, `null`, or a string.
    pub(super) fn path(&self, name: &str, out: &mut Vec<Diagnostic>) -> Field {
        match self.get(name) {
            None => Field::Absent,
            Some(Value::Null) => Field::Null,
            Some(Value::String(s)) => Field::Text(s.clone()),
            Some(_) => {
                self.wrong(name, "a STRING", out);
                Field::Absent
            }
        }
    }

    /// A number given as `Name` (a number, or a JSONata string) or as `NamePath`.
    pub(super) fn num(&self, name: &str, out: &mut Vec<Diagnostic>) -> Num {
        let path = format!("{name}Path");
        match (self.get(name), self.get(&path)) {
            (Some(Value::Number(n)), None) => n.as_f64().map_or(Num::None, Num::Fixed),
            (Some(v @ Value::String(s)), None) if crate::jsonata::is_expr(s) => {
                Num::Expr(v.clone())
            }
            (None, Some(Value::String(p))) => Num::Path(p.clone()),
            (None, None) => Num::None,
            (Some(_), Some(_)) => {
                // (memory)
                out.push(Diagnostic::error(
                    SCHEMA,
                    format!("Only one of these fields is allowed: [{name}, {path}]"),
                    &self.at,
                ));
                Num::None
            }
            (Some(_), None) => {
                self.wrong(name, "a NUMBER", out);
                Num::None
            }
            (None, Some(_)) => {
                self.wrong(&path, "a STRING", out);
                Num::None
            }
        }
    }

    /// Reports the members that are not in `allowed`.
    pub(super) fn only(&self, allowed: &[&str], out: &mut Vec<Diagnostic>) {
        for name in self.map.keys() {
            if !allowed.contains(&name.as_str()) {
                // (memory)
                out.push(Diagnostic::error(
                    SCHEMA,
                    format!("The field '{name}' is not supported by Step Functions"),
                    &self.here(name),
                ));
            }
        }
    }

    /// Reports the members of `required` that are missing, in one line.
    pub(super) fn require(&self, required: &[&str], out: &mut Vec<Diagnostic>) -> bool {
        let missing: Vec<&str> = required
            .iter()
            .copied()
            .filter(|n| !self.map.contains_key(*n))
            .collect();
        if missing.is_empty() {
            return true;
        }
        out.push(Diagnostic::error(
            SCHEMA,
            format!("These fields are required: [{}]", missing.join(", ")),
            &self.at,
        ));
        false
    }
}
