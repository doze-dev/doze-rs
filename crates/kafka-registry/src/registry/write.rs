//! Registering a schema under a subject, and finding the version a schema is registered as.
//!
//! A schema's contract (its metadata and rule set) is part of it, as Confluent's `register` makes it: the registration's
//! own, else the subject's latest version's, laid over the configuration's defaults and under its overrides
//! (`ContractConfig::apply`). A registration whose schema then has a contract is answered with the schema (Confluent's
//! `RegisterSchemaResponse` with its version, metadata and text), one without with its id alone.

use super::{
    Contract, Mode, Origin, Registry, SchemaInput, SchemaView, State, Version, check_subject,
    fingerprint,
};
use crate::error::{RegistryError, code};
use crate::store::{Batch, Key, SchemaRecord, VersionRecord};

/// The answer to a registration: the schema's id; the schema (a contract made it, or the request carried no schema and
/// it is the subject's latest).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegisterResult {
    Id(u32),
    Latest(SchemaView),
    /// The schema registered (or found) with the contract it was given, as Confluent answers one with metadata.
    Full(SchemaView),
}

impl RegisterResult {
    /// The schema's id, however it is answered.
    #[must_use]
    pub fn id(&self) -> u32 {
        match self {
            Self::Id(id) => *id,
            Self::Latest(v) | Self::Full(v) => v.id,
        }
    }
}

/// A registration as the request asks it.
struct Req<'a> {
    subject: &'a str,
    input: &'a SchemaInput,
    mode: Mode,
    origin: &'a Origin,
    /// The schema's contract once inherited and laid over the configuration's.
    contract: Contract,
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
                    .is_some_and(|r| r.print() == *print)
        })
    }

    /// The contract of a subject's latest live version.
    fn latest_contract(&self, subject: &str) -> Option<&Contract> {
        let v = self.live(subject).last()?;
        self.schemas.get(&v.rec.id).map(|r| &r.contract)
    }
}

impl Registry {
    /// Stores a schema under a subject and returns its id. A schema the subject already holds is not stored again;
    /// anything else must be compatible with the subject's history at the level in force. In IMPORT mode the schema
    /// takes the id (and the version) the request gives, unchecked, with the contract it gives.
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
        let config = s.contract_config(subject);
        if mode.read_only() {
            // A schema the subject already holds is answered with its id, as Confluent does (recorded:
            // readonly-subject): a serializer that registers before every first send keeps working. Anything else is
            // refused, whether or not it parses.
            let refused = RegistryError::read_only(subject);
            return match self
                .held_as_sent(&s, subject, input, normalize)
                .map_err(|_| refused.clone())?
            {
                Some(id) => Ok(RegisterResult::Id(id)),
                None => Err(refused),
            };
        }
        // Confluent's `registerOrForward` first looks the schema up as it was sent, unless defaults or overrides would
        // change it: a schema the subject holds (with the contract it was sent with) is its id.
        if !config.any()
            && input.version != Some(-1)
            && input.given_version().is_none()
            && let Some(id) = self.held_as_sent(&s, subject, input, normalize)?
            && input.given_id().is_none_or(|g| g == id)
        {
            return Ok(RegisterResult::Id(id));
        }
        let (p, canonical) = self.prepare(&s, subject, input, normalize)?;
        let next_number = s
            .versions(subject)
            .last()
            .map_or(1, |v| v.number.saturating_add(1));
        let contract = if mode == Mode::Import {
            input.contract.clone()
        } else {
            config.apply(
                &input.contract,
                s.latest_contract(subject),
                Some(next_number),
                input.version.is_some_and(|v| v != 0),
            )
        };
        // Confluent answers with the schema when its contract was filled in (Import fills nothing in).
        let full = mode != Mode::Import && !contract.is_none();
        let req = Req {
            subject,
            input,
            mode,
            origin,
            contract,
        };
        let id = self.register_locked(&mut s, &req, &p, canonical)?;
        if !full {
            return Ok(RegisterResult::Id(id));
        }
        let print = s.schemas.get(&id).map(SchemaRecord::print);
        let held = print.and_then(|p| s.holding(subject, &p, true));
        Ok(match held {
            Some(v) => {
                let mut view = s.view(subject, v);
                view.subject = String::new();
                RegisterResult::Full(view)
            }
            None => RegisterResult::Id(id),
        })
    }

    /// The id of the live version of a subject holding the schema as it was sent (its contract as given).
    fn held_as_sent(
        &self,
        s: &State,
        subject: &str,
        input: &SchemaInput,
        normalize: bool,
    ) -> Result<Option<u32>, RegistryError> {
        let (p, canonical) = self.prepare(s, subject, input, normalize)?;
        let print = fingerprint(p.kind, &canonical, &p.refs, &input.contract);
        Ok(s.holding(subject, &print, true).map(|v| v.rec.id))
    }

    fn register_locked(
        &self,
        s: &mut State,
        r: &Req<'_>,
        p: &super::Prepared,
        canonical: String,
    ) -> Result<u32, RegistryError> {
        let print = fingerprint(p.kind, &canonical, &p.refs, &r.contract);
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

    /// IMPORT (recorded by tools/registry-diff): the id the request gives, which may not name another schema (another
    /// contract is another schema); its version, or the next; no compatibility check.
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
            && rec.print() != *print
        {
            return Err(not_permitted(format!(
                "Overwrite new schema with id {id} is not permitted."
            )));
        }
        Ok((id, r.input.given_version().unwrap_or(next)))
    }

    /// Outside IMPORT: no id may be given, the schema must fit the subject's history (of its compatibility group, when
    /// the configuration names one), and a version given must be the next.
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
        let against = s.compared_at(subject, level, &r.contract);
        let messages: Vec<String> = self
            .findings(s, p, level, subject, &against)?
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
            contract: r.contract.clone(),
        };
        let next_id = s.next_id.max(id.saturating_add(1));
        if !known {
            b.put_schema(id, &record);
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

    /// The version of a subject that holds this schema (the newest, if several do), with the contract it is sent with;
    /// failing that, with the contract a registration would give it (the subject's latest's, laid over the
    /// configuration's defaults and under its overrides), as Confluent 8.x looks it up (7.7.1 does not try again).
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
        let found = |contract: &Contract| {
            let print = fingerprint(p.kind, &canonical, &p.refs, contract);
            s.holding(subject, &print, !deleted)
                .map(|v| s.view(subject, v))
        };
        if let Some(v) = found(&input.contract) {
            return Ok(v);
        }
        if let Some(prev) = s.latest_contract(subject) {
            let inherited = s
                .contract_config(subject)
                .apply(&input.contract, Some(prev), None, false);
            if inherited != input.contract
                && let Some(v) = found(&inherited)
            {
                return Ok(v);
            }
        }
        Err(RegistryError::schema_not_found())
    }
}
