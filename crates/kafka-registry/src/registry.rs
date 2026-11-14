//! The registry: subjects and their versions, schemas by a global id that is never given twice, the compatibility level
//! and mode of each subject and of the whole registry, and the rules around them, as Confluent's registry keeps them.
//!
//! All of it is held in memory behind one lock and written through to a [`Storage`] before a change is visible: a change
//! the store refused leaves the registry as it was. The lock is held for the whole of an operation, parsing included, so
//! two registrations of one schema cannot both take a new id.

mod check;
mod delete;
mod doze;
mod read;
mod settings;
mod write;

use std::collections::{BTreeMap, HashMap};
use std::sync::{Mutex, MutexGuard, PoisonError};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub use check::CompatFinding;
pub(crate) use doze::rfc3339_now;
pub use doze::{Overview, Resolved, ResolvedSchema, SubjectState, VersionInfo};
pub use read::VersionRef;
pub use settings::{Level, Mode};
pub use write::RegisterResult;

use crate::error::RegistryError;
use crate::format::{Formats, ParsedSchema, ResolvedRef, SchemaType};
use crate::json;
use crate::store::{Key, Reference, SchemaRecord, Setting, Storage, VersionRecord, decode};

/// The longest subject the registry stores. Confluent's HTTP server refuses a path past about 8 KiB before its registry
/// sees it; this one takes 16 KiB and refuses more as a bad request, before any work.
pub const MAX_SUBJECT_LEN: usize = 16 << 10;

/// The most distinct schemas one reference graph may name.
pub const MAX_RESOLVED_SCHEMAS: usize = 1000;

/// Who a request came from, as HTTP says: the user it signed in as, its address and its User-Agent.
#[derive(Debug, Clone, Default)]
pub struct Origin {
    pub user: String,
    pub from: String,
    pub agent: String,
}

/// A schema as a client submits it.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct SchemaInput {
    pub schema: Option<String>,
    #[serde(rename = "schemaType")]
    pub schema_type: Option<String>,
    pub references: Option<Vec<Reference>>,
}

impl SchemaInput {
    /// A schema of `kind` with no references.
    #[must_use]
    pub fn of(kind: SchemaType, text: &str) -> Self {
        Self {
            schema: Some(text.to_owned()),
            schema_type: Some(kind.name().to_owned()),
            references: None,
        }
    }

    fn text(&self) -> &str {
        self.schema.as_deref().unwrap_or_default()
    }

    fn refs(&self) -> &[Reference] {
        self.references.as_deref().unwrap_or_default()
    }
}

/// A stored schema as the API reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SchemaView {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub subject: String,
    #[serde(skip_serializing_if = "is_zero")]
    pub version: u32,
    #[serde(skip_serializing_if = "is_zero")]
    pub id: u32,
    #[serde(rename = "schemaType", skip_serializing_if = "Option::is_none")]
    pub schema_type: Option<SchemaType>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub references: Vec<Reference>,
    pub schema: String,
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's skip_serializing_if passes a reference"
)]
fn is_zero(n: &u32) -> bool {
    *n == 0
}

/// How the API names a schema's type: Avro, the default, is left out.
fn type_field(kind: SchemaType) -> Option<SchemaType> {
    (kind != SchemaType::Avro).then_some(kind)
}

/// A version of a subject.
#[derive(Debug, Clone)]
struct Version {
    number: u32,
    rec: VersionRecord,
}

/// What the registry holds.
#[derive(Debug, Default)]
struct State {
    schemas: BTreeMap<u32, SchemaRecord>,
    /// Fingerprint of (type, stored text, references) to the id.
    by_print: HashMap<[u8; 32], u32>,
    /// Each subject's versions, oldest first. A subject with none is not kept.
    subjects: BTreeMap<String, Vec<Version>>,
    levels: BTreeMap<String, Setting>,
    modes: BTreeMap<String, Setting>,
    next_id: u32,
}

/// The schema store and the rules around it. Safe for concurrent use.
#[derive(Debug)]
pub struct Registry {
    state: Mutex<State>,
    store: Box<dyn Storage>,
    formats: Formats,
    clock: fn() -> i64,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

fn fingerprint(kind: SchemaType, text: &str, refs: &[Reference]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(kind.name().as_bytes());
    h.update([0]);
    h.update(text.as_bytes());
    for r in refs {
        h.update(format!("\0{}\0{}\0{}", r.name, r.subject, r.version).as_bytes());
    }
    h.finalize().into()
}

impl Registry {
    /// Loads the registry from `store`, with every language this build reads.
    ///
    /// # Errors
    /// The store cannot be read, or holds a record this version cannot read (named in the message).
    pub fn open(store: Box<dyn Storage>) -> Result<Self, String> {
        Self::open_with(store, Formats::all())
    }

    /// [`Registry::open`] with the languages given.
    ///
    /// # Errors
    /// As [`Registry::open`].
    pub fn open_with(store: Box<dyn Storage>, formats: Formats) -> Result<Self, String> {
        let mut s = State {
            next_id: 1,
            ..State::default()
        };
        let records = store
            .load()
            .map_err(|e| format!("schema registry: load: {e}"))?;
        for (raw, value) in records {
            let bad = |e: String| {
                format!(
                    "schema registry: record {:?}: {e}",
                    String::from_utf8_lossy(&raw)
                )
            };
            match Key::decode(&raw) {
                Some(Key::Schema(id)) => {
                    let rec: SchemaRecord = decode(&value).map_err(bad)?;
                    s.by_print
                        .insert(fingerprint(rec.kind, &rec.text, &rec.refs), id);
                    s.schemas.insert(id, rec);
                }
                Some(Key::Version(subject, number)) => {
                    let rec = decode(&value).map_err(bad)?;
                    s.subjects
                        .entry(subject)
                        .or_default()
                        .push(Version { number, rec });
                }
                Some(Key::Level(subject)) => {
                    s.levels.insert(subject, decode(&value).map_err(bad)?);
                }
                Some(Key::Mode(subject)) => {
                    s.modes.insert(subject, decode(&value).map_err(bad)?);
                }
                Some(Key::NextId) => s.next_id = decode(&value).map_err(bad)?,
                None => return Err(bad("not a key this version knows".into())),
            }
        }
        for vs in s.subjects.values_mut() {
            vs.sort_by_key(|v| v.number);
        }
        if let Some((&last, _)) = s.schemas.last_key_value() {
            s.next_id = s.next_id.max(last.saturating_add(1));
        }
        Ok(Self {
            state: Mutex::new(s),
            store,
            formats,
            clock: now_ms,
        })
    }

    /// The languages this registry reads, in the order the API lists them.
    #[must_use]
    pub fn kinds(&self) -> Vec<SchemaType> {
        self.formats.kinds()
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn commit(&self, b: &crate::store::Batch) -> Result<(), RegistryError> {
        if b.is_empty() {
            return Ok(());
        }
        self.store
            .apply(&b.puts, &b.deletes)
            .map_err(RegistryError::store)
    }
}

/// A schema read for a comparison: its language and what the language made of it.
struct Prepared {
    kind: SchemaType,
    parsed: Box<dyn ParsedSchema>,
}

/// A subject Confluent refuses (a control character, or the name it keeps for the global level), and one too long to
/// store, refused without repeating it.
fn check_subject(subject: &str) -> Result<(), RegistryError> {
    use crate::error::code::INVALID_SUBJECT;
    if subject.len() > MAX_SUBJECT_LEN {
        return Err(RegistryError::new(
            422,
            INVALID_SUBJECT,
            format!(
                "The specified subject is not valid: it is {} bytes long, and the longest this registry stores is {MAX_SUBJECT_LEN}",
                subject.len()
            ),
        ));
    }
    if subject == "__GLOBAL" || subject.chars().any(|c| c < ' ' || c == '\u{7f}') {
        return Err(RegistryError::new(
            422,
            INVALID_SUBJECT,
            format!("The specified subject '{subject}' is not a valid."),
        ));
    }
    Ok(())
}

impl State {
    fn versions(&self, subject: &str) -> &[Version] {
        self.subjects.get(subject).map_or(&[], Vec::as_slice)
    }

    /// A subject's versions that are not deleted, oldest first.
    fn live(&self, subject: &str) -> impl Iterator<Item = &Version> {
        self.versions(subject).iter().filter(|v| !v.rec.deleted)
    }

    fn exists(&self, subject: &str) -> bool {
        self.live(subject).next().is_some()
    }

    fn find(&self, subject: &str, number: u32) -> Option<&Version> {
        self.versions(subject).iter().find(|v| v.number == number)
    }

    fn view(&self, subject: &str, v: &Version) -> SchemaView {
        let s = self.schemas.get(&v.rec.id);
        SchemaView {
            subject: subject.to_owned(),
            version: v.number,
            id: v.rec.id,
            schema_type: s.and_then(|s| type_field(s.kind)),
            references: s.map(|s| s.refs.clone()).unwrap_or_default(),
            schema: s.map(|s| s.text.clone()).unwrap_or_default(),
        }
    }

    /// Follows references to the schemas they name, dependencies first. Each (subject, version) is followed once, so a
    /// graph that names one schema from many places (a diamond) costs its size and not its paths. A cycle cannot be
    /// built: a reference names a version that existed before the schema naming it.
    // fuzz:parser registry_api
    fn resolve(&self, refs: &[Reference]) -> Result<Vec<ResolvedRef>, RegistryError> {
        let mut walk = Walk {
            out: Vec::new(),
            followed: Vec::new(),
        };
        walk.refs(self, refs, true)?;
        Ok(walk.out)
    }
}

struct Walk {
    out: Vec<ResolvedRef>,
    followed: Vec<String>,
}

fn node_of(r: &Reference) -> String {
    format!("{}\0{}", r.subject, r.version)
}

impl Walk {
    fn refs(&mut self, s: &State, refs: &[Reference], direct: bool) -> Result<(), RegistryError> {
        for r in refs {
            let found = u32::try_from(r.version)
                .ok()
                .and_then(|n| s.find(&r.subject, n));
            let Some(v) = found.filter(|v| !v.rec.deleted) else {
                return Err(RegistryError::invalid_schema(format!(
                    "Invalid schema: reference \"{}\": subject '{}' version {} was not found",
                    r.name, r.subject, r.version
                )));
            };
            let Some(rec) = s.schemas.get(&v.rec.id) else {
                continue;
            };
            let node = node_of(r);
            if !self.followed.contains(&node) {
                if self.followed.len() >= MAX_RESOLVED_SCHEMAS {
                    return Err(RegistryError::invalid_schema(format!(
                        "Invalid schema: references name more than {MAX_RESOLVED_SCHEMAS} schemas"
                    )));
                }
                self.followed.push(node.clone());
                self.refs(s, &rec.refs, false)?;
            }
            if let Some(seen) = self
                .out
                .iter_mut()
                .find(|o| o.node == node && o.name == r.name)
            {
                seen.direct |= direct;
                continue;
            }
            self.out.push(ResolvedRef {
                name: r.name.clone(),
                subject: r.subject.clone(),
                version: v.number,
                kind: rec.kind,
                text: rec.text.clone(),
                direct,
                node,
                names: rec
                    .refs
                    .iter()
                    .map(|x| (x.name.clone(), node_of(x)))
                    .collect(),
            });
        }
        Ok(())
    }
}

impl Registry {
    /// Reads a submitted schema: its language, what the language made of it, and the text to store.
    fn prepare(
        &self,
        s: &State,
        input: &SchemaInput,
        normalize: bool,
    ) -> Result<(Prepared, String), RegistryError> {
        let typ = input.schema_type.as_deref().unwrap_or_default();
        let kind = SchemaType::named(typ)
            .ok_or_else(|| RegistryError::invalid_schema(format!("Invalid schema type {typ}")))?;
        let format = self.formats.get(kind).ok_or_else(|| {
            RegistryError::invalid_schema(format!(
                "Invalid schema: {} schemas are not read by this build",
                kind.name()
            ))
        })?;
        let text = input.text();
        if text.trim().is_empty() {
            return Err(RegistryError::invalid_schema(
                "Invalid schema: schema may not be empty",
            ));
        }
        if format.bounded_depth() && json::depth(text) > json::MAX_SCHEMA_DEPTH {
            // Said without the schema: echoing megabytes of brackets helps nobody.
            return Err(RegistryError::invalid_schema(format!(
                "Invalid schema: nested more than {} levels deep",
                json::MAX_SCHEMA_DEPTH
            )));
        }
        let refs = s.resolve(input.refs())?;
        let parsed = format.parse(text, &refs).map_err(|why| {
            RegistryError::invalid_schema(format!(
                "Invalid schema {}, details: {why}",
                describe(input, kind)
            ))
        })?;
        let canonical = parsed.canonical(normalize);
        Ok((Prepared { kind, parsed }, canonical))
    }

    /// Reads a stored schema again, to compare with.
    fn parse_stored(&self, s: &State, id: u32) -> Result<Prepared, String> {
        let rec = s.schemas.get(&id).ok_or("no such schema")?;
        let refs = s.resolve(&rec.refs).map_err(|e| e.message)?;
        let format = self
            .formats
            .get(rec.kind)
            .ok_or("its language is not read by this build")?;
        Ok(Prepared {
            kind: rec.kind,
            parsed: format.parse(&rec.text, &refs)?,
        })
    }
}

/// The schema as Confluent's message quotes it.
fn describe(input: &SchemaInput, kind: SchemaType) -> String {
    let refs = input
        .refs()
        .iter()
        .map(|r| {
            format!(
                "io.confluent.kafka.schemaregistry.client.rest.entities.SchemaReference@0{{name={}, subject={}, version={}}}",
                r.name, r.subject, r.version
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "{{schema={}, schemaType={}, references=[{refs}]}}",
        input.text(),
        kind.name()
    )
}

#[cfg(test)]
mod tests;
