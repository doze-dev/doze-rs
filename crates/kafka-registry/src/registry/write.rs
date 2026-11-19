//! Registering a schema under a subject, and finding the version a schema is registered as.

use super::{
    Mode, Origin, Registry, SchemaInput, SchemaView, State, Version, check_subject, fingerprint,
};
use crate::error::{RegistryError, code};
use crate::store::{Batch, Key, SchemaRecord, VersionRecord};

/// The answer to a registration: the schema's id, or, when the request carried no schema, the subject's latest version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegisterResult {
    Id(u32),
    Latest(SchemaView),
}

/// A registration as the request asks it.
struct Req<'a> {
    subject: &'a str,
    input: &'a SchemaInput,
    mode: Mode,
    origin: &'a Origin,
}

fn not_permitted(message: String) -> RegistryError {
    RegistryError::new(422, code::OPERATION_NOT_PERMITTED, message)
}

impl State {
    /// The newest version of a subject holding the schema with this fingerprint (`live_only`: not a soft-deleted one).
    fn holding(&self, subject: &str, print: &[u8; 32], live_only: bool) -> Option<&Version> {
        self.versions(subject).iter().rev().find(|v| {
            (!live_only || !v.rec.deleted)
                && self
                    .schemas
                    .get(&v.rec.id)
                    .is_some_and(|r| fingerprint(r.kind, &r.text, &r.refs) == *print)
        })
    }
}

impl Registry {
    /// Stores a schema under a subject and returns its id. A schema the subject already holds is not stored again;
    /// anything else must be compatible with the subject's history at the level in force. In IMPORT mode the schema
    /// takes the id (and the version) the request gives, unchecked.
    ///
    /// # Errors
    /// Confluent's: an invalid subject or schema, a read-only subject, an id outside IMPORT mode (or none in it), a version
    /// that is not the next, an incompatible schema, a store failure.
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
        let mode = s.enforced_mode(subject);
        if mode.read_only() {
            // A schema the subject already holds is answered with its id, as Confluent does (recorded:
            // readonly-subject): a serializer that registers before every first send keeps working. Anything else is
            // refused, whether or not it parses.
            let refused = RegistryError::read_only(subject);
            let (p, canonical) = self
                .prepare(&s, subject, input, normalize)
                .map_err(|_| refused.clone())?;
            let print = fingerprint(p.kind, &canonical, &p.refs);
            return match s.holding(subject, &print, true) {
                Some(v) => Ok(RegisterResult::Id(v.rec.id)),
                None => Err(refused),
            };
        }
        let (p, canonical) = self.prepare(&s, subject, input, normalize)?;
        let req = Req {
            subject,
            input,
            mode,
            origin,
        };
        self.register_locked(&mut s, &req, &p, canonical)
            .map(RegisterResult::Id)
    }

    fn register_locked(
        &self,
        s: &mut State,
        r: &Req<'_>,
        p: &super::Prepared,
        canonical: String,
    ) -> Result<u32, RegistryError> {
        let print = fingerprint(p.kind, &canonical, &p.refs);
        let next_number = s
            .versions(r.subject)
            .last()
            .map_or(1, |v| v.number.saturating_add(1));
        if let Some(v) = s.holding(r.subject, &print, true) {
            // A version asked for is checked even for a schema the subject holds (recorded by tools/registry-diff).
            let asked = r.input.given_version();
            if r.mode != Mode::Import && asked.is_some_and(|a| a != v.number && a != next_number) {
                return Err(RegistryError::invalid_schema(
                    "Version is not one more than previous version",
                ));
            }
            return Ok(v.rec.id);
        }
        let (id, number) = if r.mode == Mode::Import {
            Self::imported(s, r, &print, next_number)?
        } else {
            self.checked(s, r, p, next_number)?;
            (
                s.by_print.get(&print).copied().unwrap_or(s.next_id),
                next_number,
            )
        };
        self.store(s, r, p, (canonical, print), (id, number))
    }

    /// IMPORT (recorded by tools/registry-diff): the id the request gives, which may not name another schema; its version,
    /// or the next; no compatibility check.
    fn imported(
        s: &State,
        r: &Req<'_>,
        print: &[u8; 32],
        next: u32,
    ) -> Result<(u32, u32), RegistryError> {
        let Some(id) = r.input.given_id() else {
            return Err(not_permitted(format!(
                "Subject {} is not in read-write mode",
                r.subject
            )));
        };
        if let Some(rec) = s.schemas.get(&id)
            && fingerprint(rec.kind, &rec.text, &rec.refs) != *print
        {
            return Err(not_permitted(format!(
                "Overwrite new schema with id {id} is not permitted."
            )));
        }
        Ok((id, r.input.given_version().unwrap_or(next)))
    }

    /// Outside IMPORT: no id may be given, the schema must fit the subject's history, and a version given must be the next.
    fn checked(
        &self,
        s: &State,
        r: &Req<'_>,
        p: &super::Prepared,
        next: u32,
    ) -> Result<(), RegistryError> {
        let subject = r.subject;
        if r.input.given_id().is_some() {
            return Err(not_permitted(format!(
                "Subject {subject} is not in import mode"
            )));
        }
        let level = s.level_of(subject);
        let messages: Vec<String> = self
            .findings(s, p, level, subject, &s.compared_at(subject, level))?
            .first()
            .map(super::CompatFinding::messages)
            .unwrap_or_default();
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
        if r.input.given_version().is_some_and(|v| v != next) {
            return Err(RegistryError::invalid_schema(
                "Version is not one more than previous version",
            ));
        }
        Ok(())
    }

    /// Writes the version (and the schema, when it is new), then makes them visible.
    fn store(
        &self,
        s: &mut State,
        r: &Req<'_>,
        p: &super::Prepared,
        (canonical, print): (String, [u8; 32]),
        (id, number): (u32, u32),
    ) -> Result<u32, RegistryError> {
        let subject = r.subject;
        let known = s.schemas.contains_key(&id);
        // Soft-deleted copies of this schema in the subject: registered again, the schema becomes a new, latest
        // version, and those go for good. A version IMPORT gives again is replaced.
        let replaced: Vec<u32> = s
            .versions(subject)
            .iter()
            .filter(|v| v.number == number || (v.rec.deleted && v.rec.id == id))
            .map(|v| v.number)
            .collect();
        let mut b = Batch::default();
        let record = SchemaRecord {
            kind: p.kind,
            text: canonical,
            refs: p.refs.clone(),
        };
        let next_id = s.next_id.max(id.saturating_add(1));
        if !known {
            b.put(&Key::Schema(id), &record);
        }
        if next_id != s.next_id {
            b.put(&Key::NextId, &next_id);
        }
        let rec = VersionRecord {
            id,
            deleted: false,
            at: (self.clock)(),
            by: r.origin.user.clone(),
            from: r.origin.from.clone(),
            agent: r.origin.agent.clone(),
        };
        for &old in replaced.iter().filter(|&&n| n != number) {
            b.delete(&Key::Version(subject.to_owned(), old));
        }
        b.put(&Key::Version(subject.to_owned(), number), &rec);
        self.commit(&b)?;
        let versions = s.subjects.entry(subject.to_owned()).or_default();
        versions.retain(|v| !replaced.contains(&v.number));
        versions.push(Version { number, rec });
        versions.sort_by_key(|v| v.number);
        if !known {
            s.schemas.insert(id, record);
            s.by_print.entry(print).or_insert(id);
        }
        s.next_id = next_id;
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
        let (p, canonical) = self.prepare(&s, subject, input, normalize)?;
        let print = fingerprint(p.kind, &canonical, &p.refs);
        s.holding(subject, &print, !deleted)
            .map(|v| s.view(subject, v))
            .ok_or_else(RegistryError::schema_not_found)
    }
}
