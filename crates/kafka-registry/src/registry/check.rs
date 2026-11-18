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
    /// The reasons, taken apart: at FULL, the forward ones (the old schema reading the new) first.
    pub details: Vec<CompatReason>,
    /// How many of `details`, from the start, are FULL's forward reasons (0 at the other levels).
    pub forward: usize,
}

impl CompatFinding {
    /// The finding as Confluent's verbose check and its 409 write it: the reasons, then the version and its text, then
    /// the level. At FULL Confluent writes the forward reasons without the version (its two-way check appends it only
    /// to the backward ones), so a FULL finding with only forward reasons names no version (recorded by
    /// tools/registry-diff).
    #[must_use]
    pub fn messages(&self) -> Vec<String> {
        let mut out: Vec<String> = self.details.iter().map(|d| d.text.clone()).collect();
        if self.details.len() > self.forward {
            out.push(format!("{{oldSchemaVersion: {}}}", self.version));
            out.push(old_schema(&self.old));
        }
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
fn compare(newer: &Prepared, older: &Prepared, level: Level) -> (Vec<CompatReason>, usize) {
    let mut out = Vec::new();
    if level.forward() {
        out.extend(can_read(older, newer, Role::Old, Role::New));
    }
    // At FORWARD alone the forward reasons are the only ones, and Confluent names the version after them.
    let forward = if level.backward() { out.len() } else { 0 };
    if level.backward() {
        out.extend(can_read(newer, older, Role::New, Role::Old));
    }
    (out, forward)
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
    /// Compares `p` with each of `against` at `level`, newest first, as Confluent does; Confluent's answers name only the
    /// first finding (the newest version that does not fit), doze's own check all of them.
    pub(super) fn findings(
        &self,
        s: &State,
        p: &Prepared,
        level: Level,
        subject: &str,
        against: &[Version],
    ) -> Result<Vec<CompatFinding>, RegistryError> {
        if level == Level::None {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        for v in against.iter().rev() {
            let old = s
                .schemas
                .get(&v.rec.id)
                .map(|r| r.text.clone())
                .unwrap_or_default();
            // A version that cannot be read again (a schema it references is gone for good) refuses the check, as
            // Confluent refuses it: the old schema quoted as invalid (recorded by tools/registry-diff).
            let stored = self
                .parse_stored(s, v.rec.id)
                .map_err(|why| self.unreadable(s, subject, v, &why))?;
            let (details, forward) = compare(p, &stored, level);
            if !details.is_empty() {
                out.push(CompatFinding {
                    version: v.number,
                    old,
                    level,
                    details,
                    forward,
                });
            }
        }
        Ok(out)
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
        let (p, _) = self.prepare(&s, subject, input, normalize)?;
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
        let found = self.findings(&s, &p, level, subject, &against)?;
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

/// The longest old schema a finding quotes whole: Confluent cuts a longer one (counted in UTF-16 units, as Java counts).
const MAX_QUOTED_SCHEMA: usize = 10240;

/// The `{oldSchema: '...'}` line, cut as Confluent cuts it (recorded by tools/registry-diff).
fn old_schema(text: &str) -> String {
    let units: Vec<u16> = text.encode_utf16().collect();
    if units.len() <= MAX_QUOTED_SCHEMA {
        return format!("{{oldSchema: '{text}'}}");
    }
    let head = String::from_utf16_lossy(units.get(..MAX_QUOTED_SCHEMA).unwrap_or_default());
    format!("{{oldSchema: <truncated> '{head}...'}}")
}

#[cfg(test)]
mod quoted {
    #[test]
    fn a_long_old_schema_is_cut_as_confluent_cuts_it() {
        assert_eq!(super::old_schema("\"int\""), "{oldSchema: '\"int\"'}");
        let long = "x".repeat(10_241);
        let got = super::old_schema(&long);
        assert!(got.starts_with("{oldSchema: <truncated> 'xxx") && got.ends_with("x...'}"));
        assert_eq!(
            got.len(),
            "{oldSchema: <truncated> '".len() + 10_240 + "...'}".len()
        );
    }
}
