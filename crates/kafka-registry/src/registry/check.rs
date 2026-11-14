//! Compatibility: the one engine behind a registration's 409, the API's compatibility check, and the console's check at
//! any level. A new schema is compared with the versions the level calls for (the latest, or all of them when transitive);
//! at FORWARD the old schema must read the new, at BACKWARD the new must read the old, at FULL both.

use super::{Level, Prepared, Registry, SchemaInput, State, Version};
use crate::error::{RegistryError, code};
use crate::format::{CompatReason, Role};

/// One existing version a schema is not compatible with, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompatFinding {
    pub version: u32,
    /// The text of the version compared with.
    pub old: String,
    pub level: Level,
    /// The reasons, taken apart.
    pub details: Vec<CompatReason>,
}

impl CompatFinding {
    /// The finding as Confluent's verbose check and its 409 write it: the reasons, then the version, its text and the level.
    #[must_use]
    pub fn messages(&self) -> Vec<String> {
        let mut out: Vec<String> = self.details.iter().map(|d| d.text.clone()).collect();
        out.push(format!("{{oldSchemaVersion: {}}}", self.version));
        out.push(format!("{{oldSchema: '{}'}}", self.old));
        out.push(format!(
            "{{validateFields: 'false', compatibility: '{}'}}",
            self.level.name()
        ));
        out
    }
}

/// Why `reader` cannot read data written with `writer`.
fn can_read(
    reader: &Prepared,
    writer: &Prepared,
    reader_is: Role,
    writer_is: Role,
) -> Vec<CompatReason> {
    if reader.kind != writer.kind {
        return vec![CompatReason::plain(
            "SCHEMA_TYPE_CHANGED",
            format!(
                "Incompatible because of different schema type: {} vs {}",
                reader.kind.name(),
                writer.kind.name()
            ),
        )];
    }
    reader
        .parsed
        .can_read(writer.parsed.as_ref(), reader_is, writer_is)
}

/// The reasons `newer` may not follow `older` at a level's base (BACKWARD, FORWARD or FULL). At FULL the forward reasons
/// (the old schema reading the new) come first, as Confluent lists them.
fn compare(newer: &Prepared, older: &Prepared, level: Level) -> Vec<CompatReason> {
    let mut out = Vec::new();
    if level.forward() {
        out.extend(can_read(older, newer, Role::Old, Role::New));
    }
    if level.backward() {
        out.extend(can_read(newer, older, Role::New, Role::Old));
    }
    out
}

impl State {
    /// The live versions a new schema is compared with at a level: the latest, or all of them when transitive.
    pub(super) fn compared_at(&self, subject: &str, level: Level) -> Vec<Version> {
        let live: Vec<Version> = self.live(subject).cloned().collect();
        if level.transitive() {
            return live;
        }
        live.last().cloned().into_iter().collect()
    }
}

impl Registry {
    /// Compares `p` with each of `against` at `level`.
    pub(super) fn findings(
        &self,
        s: &State,
        p: &Prepared,
        level: Level,
        against: &[Version],
    ) -> Vec<CompatFinding> {
        if level == Level::None {
            return Vec::new();
        }
        let mut out = Vec::new();
        for v in against {
            let old = s
                .schemas
                .get(&v.rec.id)
                .map(|r| r.text.clone())
                .unwrap_or_default();
            let details = match self.parse_stored(s, v.rec.id) {
                Ok(stored) => compare(p, &stored, level),
                Err(e) => {
                    let d = format!("existing schema version {} cannot be read: {e}", v.number);
                    vec![CompatReason::plain("SCHEMA_UNREADABLE", d)]
                }
            };
            if !details.is_empty() {
                out.push(CompatFinding {
                    version: v.number,
                    old,
                    level,
                    details,
                });
            }
        }
        out
    }

    /// Tests a schema against a version of a subject (`Some("latest")`, a number, or `None` for the versions the level
    /// calls for), at `level` (`None`: the subject's), and says why it does not fit and which versions it was compared
    /// with. A subject that does not exist takes anything as its latest; a numbered version of one is missing.
    ///
    /// # Errors
    /// An invalid level, schema or version, or a missing version.
    pub fn check(
        &self,
        subject: &str,
        version: Option<&str>,
        input: &SchemaInput,
        normalize: bool,
        level: Option<&str>,
    ) -> Result<(Vec<u32>, Vec<CompatFinding>), RegistryError> {
        let asked = match level.filter(|l| !l.is_empty()) {
            Some(l) => Some(Level::parse(l).ok_or_else(RegistryError::invalid_compatibility)?),
            None => None,
        };
        let s = self.lock();
        let (p, _) = self.prepare(&s, input, normalize)?;
        let level = asked.unwrap_or_else(|| s.level_of(subject));
        let mut against = s.compared_at(subject, level);
        if let Some(version) = version.filter(|v| !v.is_empty()) {
            match s.version_of(subject, version, false) {
                Ok(v) => against = vec![v.clone()],
                Err(e) if e.error_code == code::SUBJECT_NOT_FOUND => {
                    // No subject: anything fits its latest, but a numbered version is missing like any other
                    // (recorded: compatibility-of-missing).
                    if let Ok(n) = version.parse::<i64>()
                        && n > 0
                    {
                        return Err(RegistryError::version_not_found(
                            u32::try_from(n).unwrap_or(u32::MAX),
                        ));
                    }
                    return Ok((Vec::new(), Vec::new()));
                }
                Err(e) => return Err(e),
            }
        }
        let found = self.findings(&s, &p, level, &against);
        Ok((against.iter().map(|v| v.number).collect(), found))
    }

    /// The level a check uses: the one asked for, or the subject's.
    #[must_use]
    pub fn level_for(&self, subject: &str, asked: Option<&str>) -> String {
        match asked.filter(|a| !a.is_empty()) {
            Some(a) => a.to_ascii_uppercase(),
            None => self.lock().level_of(subject).name().to_owned(),
        }
    }
}
