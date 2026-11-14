//! Registering a schema under a subject, and finding the version a schema is registered as.

use super::{
    Origin, Registry, SchemaInput, SchemaView, State, Version, check_subject, fingerprint,
};
use crate::error::{RegistryError, code};
use crate::store::{Batch, Key, SchemaRecord, VersionRecord};

/// The answer to a registration: the schema's id, or, when the request carried no schema, the subject's latest version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegisterResult {
    Id(u32),
    Latest(SchemaView),
}

impl Registry {
    /// Stores a schema under a subject and returns its id. A schema the subject already holds is not stored again;
    /// anything else must be compatible with the subject's history at the level in force.
    ///
    /// # Errors
    /// Confluent's: an invalid subject or schema, a read-only subject, an incompatible schema, a store failure.
    pub fn register(
        &self,
        subject: &str,
        input: &SchemaInput,
        normalize: bool,
        origin: &Origin,
    ) -> Result<RegisterResult, RegistryError> {
        if subject.is_empty() {
            return Err(RegistryError::new(
                422,
                code::INVALID_REQUEST,
                "Subject may not be empty",
            ));
        }
        check_subject(subject)?;
        let mut s = self.lock();
        if input.text().trim().is_empty() {
            // No schema in the request: the answer is the subject's latest, as Confluent gives it.
            let latest = s.live(subject).last().map(|v| s.view(subject, v));
            return latest
                .map(RegisterResult::Latest)
                .ok_or_else(|| RegistryError::invalid_schema("Empty schema"));
        }
        if s.mode_of(subject) == super::Mode::ReadOnly {
            // A schema the subject already holds is answered with its id, as Confluent does (recorded:
            // readonly-subject): a serializer that registers before every first send keeps working. Anything else is
            // refused, whether or not it parses.
            let refused = RegistryError::read_only(subject);
            let (p, canonical) = self
                .prepare(&s, input, normalize)
                .map_err(|_| refused.clone())?;
            let id = s
                .by_print
                .get(&fingerprint(p.kind, &canonical, input.refs()))
                .copied();
            return match id {
                Some(id) if s.live(subject).any(|v| v.rec.id == id) => Ok(RegisterResult::Id(id)),
                _ => Err(refused),
            };
        }
        let (p, canonical) = self.prepare(&s, input, normalize)?;
        self.register_locked(&mut s, subject, input, (&p, canonical), origin)
            .map(RegisterResult::Id)
    }

    fn register_locked(
        &self,
        s: &mut State,
        subject: &str,
        input: &SchemaInput,
        (p, canonical): (&super::Prepared, String),
        origin: &Origin,
    ) -> Result<u32, RegistryError> {
        let print = fingerprint(p.kind, &canonical, input.refs());
        let known = s.by_print.get(&print).copied();
        // Soft-deleted copies of this schema in the subject: registered again, the schema becomes a new, latest
        // version, and those go for good.
        let mut replaced = Vec::new();
        if let Some(id) = known {
            for v in s.versions(subject).iter().filter(|v| v.rec.id == id) {
                if !v.rec.deleted {
                    return Ok(id);
                }
                replaced.push(v.number);
            }
        }
        let level = s.level_of(subject);
        let messages: Vec<String> = self
            .findings(s, p, level, &s.compared_at(subject, level))
            .iter()
            .flat_map(super::CompatFinding::messages)
            .collect();
        if !messages.is_empty() {
            return Err(RegistryError::new(
                409,
                code::INCOMPATIBLE,
                format!(
                    "Schema being registered is incompatible with an earlier schema for subject \"{subject}\", details: [{}]",
                    messages.join(", ")
                ),
            ));
        }
        let mut b = Batch::default();
        let id = known.unwrap_or(s.next_id);
        let record = SchemaRecord {
            kind: p.kind,
            text: canonical,
            refs: input.refs().to_vec(),
        };
        if known.is_none() {
            b.put(&Key::Schema(id), &record);
            b.put(&Key::NextId, &id.saturating_add(1));
        }
        let number = s
            .versions(subject)
            .last()
            .map_or(1, |v| v.number.saturating_add(1));
        let rec = VersionRecord {
            id,
            deleted: false,
            at: (self.clock)(),
            by: origin.user.clone(),
            from: origin.from.clone(),
            agent: origin.agent.clone(),
        };
        b.put(&Key::Version(subject.to_owned(), number), &rec);
        for &old in &replaced {
            b.delete(&Key::Version(subject.to_owned(), old));
        }
        self.commit(&b)?;
        let versions = s.subjects.entry(subject.to_owned()).or_default();
        versions.retain(|v| !replaced.contains(&v.number));
        versions.push(Version { number, rec });
        if known.is_none() {
            s.schemas.insert(id, record);
            s.by_print.insert(print, id);
            s.next_id = id.saturating_add(1);
        }
        Ok(id)
    }

    /// The version of a subject that holds this schema (the newest, if several do).
    ///
    /// # Errors
    /// No such subject, an invalid schema, or a schema the subject does not hold.
    pub fn lookup(
        &self,
        subject: &str,
        input: &SchemaInput,
        normalize: bool,
        deleted: bool,
    ) -> Result<SchemaView, RegistryError> {
        let s = self.lock();
        if s.versions(subject).is_empty() || (!deleted && !s.exists(subject)) {
            return Err(RegistryError::subject_not_found(subject));
        }
        let (p, canonical) = self.prepare(&s, input, normalize)?;
        let Some(&id) = s
            .by_print
            .get(&fingerprint(p.kind, &canonical, input.refs()))
        else {
            return Err(RegistryError::schema_not_found());
        };
        s.versions(subject)
            .iter()
            .rev()
            .find(|v| v.rec.id == id && (deleted || !v.rec.deleted))
            .map(|v| s.view(subject, v))
            .ok_or_else(RegistryError::schema_not_found)
    }
}
