//! Reading the registry: subjects, versions, schemas by id, what references a version.

use super::{Registry, SchemaView, State, Version, type_field};
use crate::error::RegistryError;

/// A version as a path names it: a number in [1, 2^31-1], or the latest (`latest` or `-1`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionRef {
    Latest,
    Number(u32),
}

impl VersionRef {
    /// Reads a version path element.
    ///
    /// # Errors
    /// Confluent's 42202 for anything else.
    // fuzz:parser registry_api
    pub fn parse(text: &str) -> Result<Self, RegistryError> {
        if text == "latest" || text == "-1" {
            return Ok(Self::Latest);
        }
        match text.parse::<u32>() {
            Ok(n) if (1..=0x7fff_ffff).contains(&n) => Ok(Self::Number(n)),
            _ => Err(RegistryError::invalid_version(text)),
        }
    }
}

impl State {
    /// One version of a subject; `deleted` admits a soft-deleted one.
    pub(super) fn version_of(
        &self,
        subject: &str,
        version: &str,
        deleted: bool,
    ) -> Result<&Version, RegistryError> {
        let wanted = VersionRef::parse(version)?;
        if self.versions(subject).is_empty() || (!deleted && !self.exists(subject)) {
            return Err(RegistryError::subject_not_found(subject));
        }
        match wanted {
            VersionRef::Latest => {
                let latest = if deleted {
                    self.versions(subject).last()
                } else {
                    self.live(subject).last()
                };
                latest.ok_or_else(|| RegistryError::subject_not_found(subject))
            }
            VersionRef::Number(n) => self
                .find(subject, n)
                .filter(|v| deleted || !v.rec.deleted)
                .ok_or_else(|| RegistryError::version_not_found(n)),
        }
    }

    /// The ids of schemas, held by some live version, that reference a subject's version, ascending.
    pub(super) fn referencing(&self, subject: &str, version: u32) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        for v in self.subjects.values().flatten().filter(|v| !v.rec.deleted) {
            let Some(rec) = self.schemas.get(&v.rec.id) else {
                continue;
            };
            let names = rec
                .refs
                .iter()
                .any(|r| r.subject == subject && r.version == i64::from(version));
            if names && !out.contains(&v.rec.id) {
                out.push(v.rec.id);
            }
        }
        out.sort_unstable();
        out
    }
}

impl Registry {
    /// One version of a subject.
    ///
    /// # Errors
    /// An invalid version, no such subject or version.
    pub fn version(
        &self,
        subject: &str,
        version: &str,
        deleted: bool,
    ) -> Result<SchemaView, RegistryError> {
        let s = self.lock();
        let v = s.version_of(subject, version, deleted)?;
        Ok(s.view(subject, v))
    }

    /// Subject names, sorted. `prefix` limits them; `deleted` adds those whose every version is deleted; `deleted_only`
    /// lists only those.
    #[must_use]
    pub fn subjects(&self, prefix: &str, deleted: bool, deleted_only: bool) -> Vec<String> {
        let s = self.lock();
        s.subjects
            .iter()
            .filter(|(name, vs)| name.starts_with(prefix) && !vs.is_empty())
            .filter(|(name, _)| {
                let alive = s.exists(name);
                if deleted_only {
                    !alive
                } else {
                    alive || deleted
                }
            })
            .map(|(name, _)| name.clone())
            .collect()
    }

    /// A subject's version numbers.
    ///
    /// # Errors
    /// No such subject.
    pub fn versions(&self, subject: &str, deleted: bool) -> Result<Vec<u32>, RegistryError> {
        let s = self.lock();
        if s.versions(subject).is_empty() || (!deleted && !s.exists(subject)) {
            return Err(RegistryError::subject_not_found(subject));
        }
        Ok(s.versions(subject)
            .iter()
            .filter(|v| deleted || !v.rec.deleted)
            .map(|v| v.number)
            .collect())
    }

    /// A schema by its global id.
    ///
    /// # Errors
    /// No schema has that id.
    pub fn schema_by_id(&self, id: u32) -> Result<SchemaView, RegistryError> {
        let s = self.lock();
        let rec = s
            .schemas
            .get(&id)
            .ok_or_else(|| RegistryError::schema_id_not_found(id))?;
        Ok(SchemaView {
            subject: String::new(),
            version: 0,
            id: 0,
            schema_type: type_field(rec.kind),
            references: rec.refs.clone(),
            schema: rec.text.clone(),
        })
    }

    /// The highest id given out.
    #[must_use]
    pub fn max_id(&self) -> u32 {
        self.lock().next_id.saturating_sub(1)
    }

    /// The (subject, version) pairs that hold an id, sorted.
    ///
    /// # Errors
    /// No schema has that id.
    pub fn id_versions(&self, id: u32, deleted: bool) -> Result<Vec<(String, u32)>, RegistryError> {
        let s = self.lock();
        if !s.schemas.contains_key(&id) {
            return Err(RegistryError::schema_not_found());
        }
        Ok(s.subjects
            .iter()
            .flat_map(|(name, vs)| vs.iter().map(move |v| (name, v)))
            .filter(|(_, v)| v.rec.id == id && (deleted || !v.rec.deleted))
            .map(|(name, v)| (name.clone(), v.number))
            .collect())
    }

    /// Every version's schema, by subject then version (the latest live one of each subject with `latest_only`).
    #[must_use]
    pub fn all_schemas(&self, prefix: &str, latest_only: bool, deleted: bool) -> Vec<SchemaView> {
        let s = self.lock();
        let mut out = Vec::new();
        for (name, vs) in s.subjects.iter().filter(|(n, _)| n.starts_with(prefix)) {
            let keep: Vec<&Version> = vs.iter().filter(|v| deleted || !v.rec.deleted).collect();
            let keep = if latest_only {
                keep.last().copied().into_iter().collect()
            } else {
                keep
            };
            out.extend(keep.into_iter().map(|v| s.view(name, v)));
        }
        out
    }

    /// The ids of schemas that reference a version of a subject (a soft-deleted version included).
    ///
    /// # Errors
    /// An invalid version, no such subject or version.
    pub fn referenced_by(&self, subject: &str, version: &str) -> Result<Vec<u32>, RegistryError> {
        let s = self.lock();
        let v = s.version_of(subject, version, true)?;
        Ok(s.referencing(subject, v.number))
    }

    /// The ids a subject holds, deleted versions included.
    #[must_use]
    pub fn ids_of(&self, subject: &str) -> Vec<u32> {
        self.lock()
            .versions(subject)
            .iter()
            .map(|v| v.rec.id)
            .collect()
    }
}
