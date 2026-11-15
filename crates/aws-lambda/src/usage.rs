//! What each function and layer of a region holds on disk (its code packages and rows), the code nothing points at, and the
//! runtime clients, for `doze-aws data`.

use std::collections::BTreeSet;
use std::time::Duration;

use aws_wire::storage::{File, Note, Prune, Resource};
use serde_json::Value;

use crate::core::Core;
use crate::data::{CLIENTS, UNUSED, encoded, size_of};

/// A function name from an ARN (`...:function:name` or `...:function:name:qualifier`).
fn function_of(arn: &str) -> Option<&str> {
    arn.split(':').nth(6)
}

impl Core {
    /// The bytes of the code packages named by `ids`.
    fn code_bytes<'a>(&self, ids: impl IntoIterator<Item = &'a String>) -> u64 {
        ids.into_iter()
            .filter(|id| !id.is_empty())
            .map(|id| size_of(&self.code_dir().join(id)))
            .sum()
    }

    /// Each function and layer with what it holds, and what nothing uses.
    pub(crate) fn usage(&self, region: &str) -> Vec<Resource> {
        let mut mapped: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
        for (uuid, m) in self.mappings.iter() {
            let config: Value = serde_json::from_str(&m.config).unwrap_or_default();
            if let Some(name) = config
                .get("FunctionArn")
                .and_then(Value::as_str)
                .and_then(function_of)
            {
                *mapped.entry(name.to_owned()).or_default() +=
                    encoded("event source mapping", uuid, m);
            }
        }
        let resource = |name: String, bytes: u64, items: u64, notes: Vec<Note>| Resource {
            region: region.to_owned(),
            name,
            bytes,
            items,
            notes,
        };
        let mut out = Vec::new();
        for (name, f) in self.functions.iter() {
            let latest = &f.latest.code.id;
            let ids: BTreeSet<&String> = std::iter::once(latest)
                .chain(f.versions.iter().map(|v| &v.snap.code.id))
                .filter(|id| !id.is_empty())
                .collect();
            let rows = encoded("function", name, f) + mapped.get(name).copied().unwrap_or(0);
            let mut notes = Vec::new();
            let older: Vec<&String> = ids.iter().copied().filter(|id| *id != latest).collect();
            let older_bytes = self.code_bytes(older.iter().copied());
            if older_bytes > 0 {
                notes.push(Note {
                    bytes: older_bytes,
                    text: format!(
                        "in code only published versions run ({} of them): DeleteFunction with a Qualifier removes one",
                        f.versions.iter().filter(|v| v.snap.code.id != *latest).count()
                    ),
                    prune: None,
                });
            }
            let bytes = rows + self.code_bytes(ids.iter().copied());
            out.push(resource(name.to_owned(), bytes, ids.len() as u64, notes));
        }
        for (name, l) in self.layers.iter() {
            let bytes =
                encoded("layer", name, l) + self.code_bytes(l.versions.iter().map(|v| &v.code.id));
            out.push(resource(
                format!("layer:{name}"),
                bytes,
                l.versions.len() as u64,
                Vec::new(),
            ));
        }
        out.extend(self.leftovers(region));
        out
    }

    /// The code nothing uses, and the runtime clients: each a resource whose note removes it.
    fn leftovers(&self, region: &str) -> Vec<Resource> {
        let unused = Prune {
            unused: true,
            ..Prune::default()
        };
        let mut out = Vec::new();
        let code = self.unused_code(Duration::ZERO);
        let bytes: u64 = code.iter().map(|(_, b)| b).sum();
        if !code.is_empty() {
            out.push(Resource {
                region: region.to_owned(),
                name: UNUSED.to_owned(),
                bytes,
                items: code.len() as u64,
                notes: vec![Note {
                    bytes,
                    text: format!(
                        "in {} no function, version or layer points at",
                        packages(code.len())
                    ),
                    prune: Some(unused.clone()),
                }],
            });
        }
        if let Some(dir) = self.clients_dir().filter(|d| d.is_dir()) {
            let bytes = size_of(&dir);
            out.push(Resource {
                region: region.to_owned(),
                name: CLIENTS.to_owned(),
                bytes,
                items: std::fs::read_dir(&dir).map_or(0, |r| r.count() as u64),
                notes: vec![Note {
                    bytes,
                    text: "the Runtime API clients written for Python, Node and Ruby functions: written again when one next starts"
                        .to_owned(),
                    prune: Some(unused),
                }],
            });
        }
        out
    }

    /// The database file and what compacting it would give back.
    pub(crate) fn file(&self, region: &str) -> File {
        let space = self
            .db
            .as_ref()
            .and_then(|db| db.space().ok())
            .unwrap_or_default();
        File {
            region: region.to_owned(),
            bytes: space.file,
            reclaimable: space.reclaimable(),
        }
    }
}

/// `3 code packages`.
pub(crate) fn packages(n: usize) -> String {
    if n == 1 {
        "1 code package".to_owned()
    } else {
        format!("{n} code packages")
    }
}
