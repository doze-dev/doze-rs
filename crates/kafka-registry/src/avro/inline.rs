//! The resolved view of an Avro schema (`GET /_doze/.../resolved`): the schema with each type it takes from a reference
//! written out where it is first used, as Avro itself reads it (a named type is defined once and named after that).
//! Everything else is as written: docs, defaults and properties are kept.

use std::collections::{HashMap, HashSet};

use super::schema::{full_name, split_name};
use super::text::walk_defs;
use crate::json::Json;

const PRIMITIVES: [&str; 8] = [
    "null", "boolean", "int", "long", "float", "double", "bytes", "string",
];

/// `text` with the named types its references (`refs`, their texts) define written out where they are first used.
///
/// # Errors
/// A text is not JSON.
pub(crate) fn inline(text: &str, refs: &[&str]) -> Result<String, String> {
    let mut root = Json::parse(text).map_err(|e| e.to_string())?;
    let mut defs: HashMap<String, Json> = HashMap::new();
    for r in refs {
        let n = Json::parse(r).map_err(|e| e.to_string())?;
        walk_defs(&n, None, &mut |full, decl| {
            defs.insert(full.to_owned(), decl.clone());
        });
    }
    let mut w = Inliner {
        defs,
        done: HashSet::new(),
    };
    w.walk(&mut root, None);
    Ok(root.to_text())
}

struct Inliner {
    /// Full name to its definition, from the references.
    defs: HashMap<String, Json>,
    /// Named types already defined in the output.
    done: HashSet<String>,
}

impl Inliner {
    fn walk(&mut self, n: &mut Json, enclosing: Option<&str>) {
        match n {
            Json::Str(name) => {
                if let Some(def) = self.first_use(name, enclosing) {
                    *n = def;
                }
            }
            Json::Array(items) => items.iter_mut().for_each(|it| self.walk(it, enclosing)),
            Json::Object(_) => self.object(n, enclosing),
            Json::Raw(_) => {}
        }
    }

    /// The definition to write in place of a reference to `name`, when it is a referenced type not yet written.
    fn first_use(&mut self, name: &str, enclosing: Option<&str>) -> Option<Json> {
        if PRIMITIVES.contains(&name) {
            return None;
        }
        let mut full = name.to_owned();
        if !name.contains('.') {
            full = full_name(name, enclosing);
            if !self.defs.contains_key(&full) {
                full = name.to_owned(); // a name in the null namespace
            }
        }
        if self.done.contains(&full) {
            return None;
        }
        let mut def = self.defs.get(&full)?.clone();
        self.done.insert(full.clone());
        // Written where it is used, the definition carries its full name, so it means the same whatever namespace
        // encloses it.
        let space = full.rsplit_once('.').map(|(s, _)| s.to_owned());
        self.fields(&mut def, space.as_deref());
        def.set("name", Json::Str(full));
        def.remove("namespace");
        Some(def)
    }

    fn object(&mut self, n: &mut Json, enclosing: Option<&str>) {
        let Some(t) = n.get("type") else { return };
        let Json::Str(word) = t else {
            let mut inner = t.clone();
            self.walk(&mut inner, enclosing);
            n.set("type", inner);
            return;
        };
        match word.as_str() {
            "record" | "error" | "enum" | "fixed" => {
                let name = n
                    .get("name")
                    .and_then(Json::as_str)
                    .unwrap_or_default()
                    .to_owned();
                let (space, simple) = split_name(n, &name, enclosing);
                self.done.insert(full_name(&simple, space.as_deref()));
                self.fields(n, space.as_deref());
            }
            "array" | "map" => {
                let key = if word == "array" { "items" } else { "values" };
                if let Some(inner) = n.get(key) {
                    let mut inner = inner.clone();
                    self.walk(&mut inner, enclosing);
                    n.set(key, inner);
                }
            }
            _ => {}
        }
    }

    /// Walks the fields of a record whose namespace is `space`.
    fn fields(&mut self, n: &mut Json, space: Option<&str>) {
        let Json::Object(m) = n else { return };
        let Some((_, Json::Array(fields))) = m.iter_mut().find(|(k, _)| k == "fields") else {
            return;
        };
        for fld in fields {
            if let Json::Object(fm) = fld
                && let Some((_, ty)) = fm.iter_mut().find(|(k, _)| k == "type")
            {
                self.walk(ty, space);
            }
        }
    }
}
