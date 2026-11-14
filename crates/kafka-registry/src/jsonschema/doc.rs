//! The documents a JSON Schema is read with (itself and every schema its references reach), the draft each is written
//! in, and how a `$ref` in one of them finds its target.
//!
//! Each referenced schema is its own document with its own names: `x.json` in a schema and `x.json` in a schema it
//! references can be two different schemas (recorded: nested-references), as the Go edition places each at an address.

use crate::format::ResolvedRef;
use crate::json::Json;

/// The JSON Schema drafts Confluent's registry (and the Go edition's library) accepts, named by `$schema`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Draft {
    D4,
    D6,
    D7,
    D2019,
    D2020,
}

impl Draft {
    /// The draft a `$schema` names: draft 4, 6 or 7, 2019-09 or 2020-12 (over http or https, an empty fragment
    /// allowed); `json-schema.org/schema` is the latest. Anything else is not a draft this registry reads.
    fn of_url(url: &str) -> Option<Self> {
        let u = url.strip_suffix('#').unwrap_or(url);
        let u = u
            .strip_prefix("http://")
            .or_else(|| u.strip_prefix("https://"))
            .unwrap_or(u);
        match u {
            "json-schema.org/schema" | "json-schema.org/draft/2020-12/schema" => Some(Self::D2020),
            "json-schema.org/draft/2019-09/schema" => Some(Self::D2019),
            "json-schema.org/draft-07/schema" => Some(Self::D7),
            "json-schema.org/draft-06/schema" => Some(Self::D6),
            "json-schema.org/draft-04/schema" => Some(Self::D4),
            _ => None,
        }
    }

    /// The draft of a document: its `$schema`, or draft 7, Confluent's default.
    fn of_doc(doc: &Json) -> Result<Self, String> {
        match field(doc, "$schema") {
            None => Ok(Self::D7),
            Some(Json::Str(url)) => {
                Self::of_url(url).ok_or_else(|| format!("$schema \"{url}\" is not a JSON Schema draft"))
            }
            Some(_) => Err("$schema must be a string".to_owned()),
        }
    }
}

/// The value of `key` in an object, the last when it is repeated (as a JSON decoder into a map keeps it).
pub(crate) fn field<'a>(v: &'a Json, key: &str) -> Option<&'a Json> {
    match v {
        Json::Object(m) => m.iter().rev().find(|(k, _)| k == key).map(|(_, v)| v),
        _ => None,
    }
}

/// An object's members, each key once with its last value, in the order the keys first appear.
pub(crate) fn members(v: &Json) -> Vec<(&str, &Json)> {
    let mut out: Vec<(&str, &Json)> = Vec::new();
    if let Json::Object(m) = v {
        for (k, val) in m {
            match out.iter_mut().find(|(seen, _)| *seen == k.as_str()) {
                Some(slot) => slot.1 = val,
                None => out.push((k.as_str(), val)),
            }
        }
    }
    out
}

/// One document: the schema being read (index 0) or a schema a reference reaches.
#[derive(Debug)]
pub(crate) struct Doc {
    pub(crate) json: Json,
    pub(crate) draft: Draft,
    /// The reference names this document may use, and the document each reaches.
    names: Vec<(String, usize)>,
}

/// Every document a schema is read with.
#[derive(Debug)]
pub(crate) struct Docs(pub(crate) Vec<Doc>);

impl Docs {
    /// Reads `text` and the schemas `refs` reach.
    pub(crate) fn read(text: &str, refs: &[ResolvedRef]) -> Result<Self, String> {
        let root = Json::parse(text).map_err(|e| format!("Invalid JSON: {e}"))?;
        // One document per schema reached (by node), whatever names reach it.
        let mut nodes: Vec<String> = Vec::new();
        let mut first: Vec<&ResolvedRef> = Vec::new();
        let mut by_name: Vec<(String, String)> = Vec::new();
        let mut main: Vec<(String, String)> = Vec::new();
        for r in refs {
            let (node, direct) = if r.node.is_empty() {
                // Names only (a caller without the graph): every name is one schema.
                (format!("name\0{}", r.name), true)
            } else {
                (r.node.clone(), r.direct)
            };
            set(&mut by_name, &r.name, &node);
            if direct {
                set(&mut main, &r.name, &node);
            }
            if !nodes.contains(&node) {
                nodes.push(node);
                first.push(r);
            }
        }
        let index = |names: &[(String, String)]| -> Vec<(String, usize)> {
            names
                .iter()
                .filter_map(|(n, node)| {
                    let i = nodes.iter().position(|x| x == node)?;
                    Some((n.clone(), i + 1))
                })
                .collect()
        };
        let mut docs = vec![Doc {
            draft: Draft::of_doc(&root)?,
            json: root,
            names: index(&main),
        }];
        for r in first {
            let json = Json::parse(&r.text)
                .map_err(|e| format!("reference \"{}\": Invalid JSON: {e}", r.name))?;
            let names = if r.names.is_empty() { &by_name } else { &r.names };
            docs.push(Doc {
                draft: Draft::of_doc(&json).map_err(|e| format!("reference \"{}\": {e}", r.name))?,
                json,
                names: index(names),
            });
        }
        Ok(Self(docs))
    }

    pub(crate) fn doc(&self, i: usize) -> Option<&Doc> {
        self.0.get(i)
    }

    /// The target of `reference` written in document `from`: the document it is in and the value. A reference names
    /// one of the document's references (all of it, or a fragment of it) or a fragment of the document itself; a
    /// fragment is a JSON pointer or an anchor.
    pub(crate) fn resolve(&self, from: usize, reference: &str) -> Result<(usize, &Json), String> {
        let unresolved = || format!("unresolved reference \"{reference}\"");
        let (name, fragment) = match reference.split_once('#') {
            Some((n, f)) => (n, Some(f)),
            None => (reference, None),
        };
        let doc = if name.is_empty() {
            from
        } else {
            let names = &self.doc(from).ok_or_else(unresolved)?.names;
            names
                .iter()
                .rev()
                .find(|(n, _)| n == name)
                .map(|(_, i)| *i)
                .ok_or_else(unresolved)?
        };
        let root = &self.doc(doc).ok_or_else(unresolved)?.json;
        let fragment = percent_decode(fragment.unwrap_or_default()).ok_or_else(unresolved)?;
        let target = if fragment.is_empty() {
            Some(root)
        } else if let Some(pointer) = fragment.strip_prefix('/') {
            point(root, pointer)
        } else {
            anchor(root, &fragment, 0)
        };
        target.map(|t| (doc, t)).ok_or_else(unresolved)
    }
}

fn set(list: &mut Vec<(String, String)>, name: &str, node: &str) {
    match list.iter_mut().find(|(n, _)| n == name) {
        Some(slot) => node.clone_into(&mut slot.1),
        None => list.push((name.to_owned(), node.to_owned())),
    }
}

/// Follows a JSON pointer (without its leading `/`) through objects and arrays.
fn point<'a>(root: &'a Json, pointer: &str) -> Option<&'a Json> {
    let mut cur = root;
    for part in pointer.split('/') {
        let part = part.replace("~1", "/").replace("~0", "~");
        cur = match cur {
            Json::Object(_) => field(cur, &part)?,
            Json::Array(items) => {
                let canonical = part == "0" || !part.starts_with('0');
                if !canonical || part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
                    return None;
                }
                items.get(part.parse::<usize>().ok()?)?
            }
            _ => return None,
        };
    }
    Some(cur)
}

/// The subschema that declares a plain-name fragment: `$anchor` (2019-09 on) or an `$id` / `id` of `#name` (before).
fn anchor<'a>(v: &'a Json, name: &str, depth: usize) -> Option<&'a Json> {
    if depth > crate::json::MAX_SCHEMA_DEPTH {
        return None;
    }
    match v {
        Json::Object(m) => {
            let declared = |k: &str| field(v, k).and_then(Json::as_str);
            if declared("$anchor") == Some(name)
                || [declared("$id"), declared("id")]
                    .into_iter()
                    .flatten()
                    .any(|id| id.strip_prefix('#') == Some(name))
            {
                return Some(v);
            }
            m.iter().find_map(|(_, x)| anchor(x, name, depth + 1))
        }
        Json::Array(items) => items.iter().find_map(|x| anchor(x, name, depth + 1)),
        _ => None,
    }
}

/// A fragment's `%XX` escapes, as a URI fragment is read; `None` when one is malformed or not UTF-8.
fn percent_decode(s: &str) -> Option<String> {
    if !s.contains('%') {
        return Some(s.to_owned());
    }
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while let Some(&c) = b.get(i) {
        if c == b'%' {
            let hex = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(c);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}
