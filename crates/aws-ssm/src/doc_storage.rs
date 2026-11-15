//! The documents' and settings' part of `doze-aws data`. A document grows by its versions (`UpdateDocument` keeps every one, up to
//! 1,000): one with more than one version is listed as `document/<name>`, and `--old-versions` removes the versions that are
//! neither its default nor its latest (the two a caller resolves without naming a number). A reset removes every document and
//! every changed service setting (a setting then reads as its default).

use aws_wire::storage::{Emptied, Note, Prune, Resource, StorageError};
use doze_store::record;

use crate::core::{Core, VERSION};
use crate::document::{DocVersion, Document};

/// How a document is named in `doze-aws data`, so it is not taken for a parameter.
pub(crate) const PREFIX: &str = "document/";

fn doc_bytes(name: &str, d: &Document) -> u64 {
    (name.len() + record::encode("document", VERSION, d).map_or(0, |b| b.len())) as u64
}

fn version_bytes(v: &DocVersion) -> u64 {
    record::encode("document", VERSION, v).map_or(0, |b| b.len() as u64)
}

/// The versions an `--old-versions` prune removes.
fn removable(d: &Document) -> impl Iterator<Item = &DocVersion> {
    let latest = d.latest().map_or(0, |v| v.number);
    d.versions
        .iter()
        .filter(move |v| v.number != latest && v.number != d.default_version)
}

impl Core {
    pub(crate) fn document_usage(&self, region: &str) -> Vec<Resource> {
        self.documents
            .iter()
            .filter(|(_, d)| d.versions.len() > 1)
            .map(|(name, d)| {
                let old: Vec<&DocVersion> = removable(d).collect();
                let notes = if old.is_empty() {
                    Vec::new()
                } else {
                    vec![Note {
                        bytes: old.iter().map(|v| version_bytes(v)).sum(),
                        text: format!(
                            "in {} neither default nor latest",
                            crate::storage::plural(old.len(), "old version")
                        ),
                        prune: Some(Prune {
                            old_versions: true,
                            ..Prune::default()
                        }),
                    }]
                };
                Resource {
                    region: region.to_owned(),
                    name: format!("{PREFIX}{name}"),
                    bytes: doc_bytes(name, d),
                    items: d.versions.len() as u64,
                    notes,
                }
            })
            .collect()
    }

    /// Empties document `name` (without its prefix): its old versions.
    pub(crate) fn empty_document(
        &mut self,
        name: &str,
        prune: &Prune,
    ) -> Result<Emptied, StorageError> {
        let d = self.documents.get(name).ok_or(StorageError::NotFound)?;
        let mut done = Emptied::default();
        let gone: Vec<i64> = removable(d)
            .map(|v| {
                done.items += 1;
                done.bytes += version_bytes(v);
                v.number
            })
            .collect();
        if prune.dry_run || gone.is_empty() {
            return Ok(done);
        }
        if let Some(d) = self.documents.get_mut(name) {
            d.versions.retain(|v| !gone.contains(&v.number));
        }
        Ok(done)
    }

    /// Counts, and unless `dry_run` removes, every document and changed setting.
    pub(crate) fn reset_documents(&mut self, dry_run: bool, done: &mut Emptied) {
        for (name, d) in self.documents.iter() {
            done.items += d.versions.len() as u64;
            done.bytes += doc_bytes(name, d);
        }
        for (id, s) in self.settings.iter() {
            done.items += 1;
            done.bytes +=
                (id.len() + record::encode("setting", VERSION, s).map_or(0, |b| b.len())) as u64;
        }
        if dry_run {
            return;
        }
        let names: Vec<String> = self.documents.iter().map(|(k, _)| k.to_owned()).collect();
        for name in names {
            self.documents.remove(&name);
        }
        let ids: Vec<String> = self.settings.iter().map(|(k, _)| k.to_owned()).collect();
        for id in ids {
            self.settings.remove(&id);
        }
    }
}
