//! What the console needs that Confluent's API does not carry: who registered a version and when, when a level or mode
//! was set, the whole registry in one answer, and a schema with its references written out. Served under `/_doze/`, which
//! is not a Confluent path.

use serde::Serialize;

use super::{Registry, SchemaView, State};
use crate::error::RegistryError;
use crate::format::SchemaType;

/// Now, as [`rfc3339`] writes it.
pub(crate) fn rfc3339_now() -> String {
    rfc3339(super::now_ms())
}

/// Unix milliseconds as RFC 3339 UTC with millisecond precision; `None` for 0 (never recorded).
fn when(ms: i64) -> Option<String> {
    (ms != 0).then(|| rfc3339(ms))
}

/// Civil time from Unix milliseconds (Howard Hinnant's days-from-civil, inverted).
fn rfc3339(ms: i64) -> String {
    let (secs, milli) = (ms.div_euclid(1000), ms.rem_euclid(1000));
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let (h, m, s) = (rem / 3600, rem % 3600 / 60, rem % 60);
    if milli == 0 {
        format!("{year:04}-{month:02}-{day:02}T{h:02}:{m:02}:{s:02}Z")
    } else {
        // As Go writes a time: the fraction without its trailing zeros.
        let fraction = format!("{milli:03}");
        format!(
            "{year:04}-{month:02}-{day:02}T{h:02}:{m:02}:{s:02}.{}Z",
            fraction.trim_end_matches('0')
        )
    }
}

/// A version with what the registry knows about how it came to be.
#[derive(Debug, Clone, Serialize)]
pub struct VersionInfo {
    #[serde(flatten)]
    pub view: SchemaView,
    pub deleted: bool,
    #[serde(rename = "registeredAt")]
    pub registered_at: Option<String>,
    #[serde(rename = "registeredBy", skip_serializing_if = "String::is_empty")]
    pub registered_by: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub from: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub agent: String,
}

/// The registry at a glance.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub next_id: u32,
    /// The ids that resolve, as runs `[first, last]`.
    pub ids_in_use: Vec<[u32; 2]>,
    /// The ids a version that is not deleted holds, as runs.
    pub live_ids: Vec<[u32; 2]>,
    pub level: String,
    pub level_set_at: Option<String>,
    pub mode: String,
    pub mode_set_at: Option<String>,
    pub subjects: Vec<SubjectState>,
}

/// One subject in the overview, a soft-deleted one included.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubjectState {
    pub subject: String,
    /// Of the latest version.
    #[serde(rename = "type")]
    pub kind: Option<SchemaType>,
    pub versions: u32,
    pub deleted_versions: u32,
    /// Every version soft-deleted.
    pub deleted: bool,
    pub latest_version: u32,
    pub latest_id: u32,
    pub level: String,
    pub level_is_own: bool,
    pub level_set_at: Option<String>,
    pub mode: String,
    pub mode_is_own: bool,
    pub mode_set_at: Option<String>,
    pub last_registered: Option<String>,
}

/// A version with what its references bring in.
#[derive(Debug, Clone, Serialize)]
pub struct Resolved {
    #[serde(rename = "schemaType")]
    pub kind: SchemaType,
    pub schema: String,
    /// Whether `schema` has the referenced types written out.
    pub inlined: bool,
    pub references: Vec<ResolvedSchema>,
}

/// One schema a version reaches through its references.
#[derive(Debug, Clone, Serialize)]
pub struct ResolvedSchema {
    pub name: String,
    pub subject: String,
    pub version: u32,
    pub schema: String,
}

/// Sorted ids as runs `[first, last]`.
fn runs(ids: impl IntoIterator<Item = u32>) -> Vec<[u32; 2]> {
    let mut out: Vec<[u32; 2]> = Vec::new();
    for id in ids {
        match out.last_mut() {
            Some(run) if run[1].checked_add(1) == Some(id) => run[1] = id,
            _ => out.push([id, id]),
        }
    }
    out
}

impl State {
    fn subject_state(&self, name: &str) -> Option<SubjectState> {
        let vs = self.versions(name);
        let latest = vs
            .iter()
            .rev()
            .find(|v| !v.rec.deleted)
            .or_else(|| vs.last())?;
        let live = u32::try_from(vs.iter().filter(|v| !v.rec.deleted).count()).unwrap_or(u32::MAX);
        let all = u32::try_from(vs.len()).unwrap_or(u32::MAX);
        let level = self.levels.get(name);
        let mode = self.modes.get(name);
        Some(SubjectState {
            subject: name.to_owned(),
            kind: self.schemas.get(&latest.rec.id).map(|s| s.kind),
            versions: live,
            deleted_versions: all - live,
            deleted: live == 0,
            latest_version: latest.number,
            latest_id: latest.rec.id,
            level: self.level_of(name).name().to_owned(),
            level_is_own: level.is_some(),
            level_set_at: level.and_then(|l| when(l.at)),
            mode: self.mode_of(name).name().to_owned(),
            mode_is_own: mode.is_some(),
            mode_set_at: mode.and_then(|m| when(m.at)),
            last_registered: when(vs.iter().map(|v| v.rec.at).max().unwrap_or(0)),
        })
    }
}

impl Registry {
    /// One version of a subject, a soft-deleted one included, with who registered it and when.
    ///
    /// # Errors
    /// An invalid version, no such subject or version.
    pub fn version_info(&self, subject: &str, version: &str) -> Result<VersionInfo, RegistryError> {
        let s = self.lock();
        let v = s.version_of(subject, version, true)?;
        Ok(VersionInfo {
            view: s.view(subject, v),
            deleted: v.rec.deleted,
            registered_at: when(v.rec.at),
            registered_by: v.rec.by.clone(),
            from: v.rec.from.clone(),
            agent: v.rec.agent.clone(),
        })
    }

    /// The whole registry, subjects by name.
    #[must_use]
    pub fn overview(&self) -> Overview {
        let s = self.lock();
        let mut live: Vec<u32> = s
            .subjects
            .values()
            .flatten()
            .filter(|v| !v.rec.deleted)
            .map(|v| v.rec.id)
            .collect();
        live.sort_unstable();
        live.dedup();
        let global_level = s.levels.get("");
        let global_mode = s.modes.get("");
        Overview {
            next_id: s.next_id,
            ids_in_use: runs(s.schemas.keys().copied()),
            live_ids: runs(live),
            level: s.level_of("").name().to_owned(),
            level_set_at: global_level.and_then(|l| when(l.at)),
            mode: s.mode_of("").name().to_owned(),
            mode_set_at: global_mode.and_then(|m| when(m.at)),
            subjects: s
                .subjects
                .keys()
                .filter_map(|name| s.subject_state(name))
                .collect(),
        }
    }

    /// One version with the schemas its references reach; for Avro, with the referenced types written out.
    ///
    /// # Errors
    /// An invalid version, no such subject or version, a reference that no longer resolves.
    pub fn resolved(&self, subject: &str, version: &str) -> Result<Resolved, RegistryError> {
        let s = self.lock();
        let v = s.version_of(subject, version, true)?;
        let rec = s
            .schemas
            .get(&v.rec.id)
            .ok_or_else(RegistryError::schema_not_found)?;
        let refs = s.resolve(&rec.refs)?;
        let mut out = Resolved {
            kind: rec.kind,
            schema: rec.text.clone(),
            inlined: false,
            references: refs
                .iter()
                .map(|r| ResolvedSchema {
                    name: r.name.clone(),
                    subject: r.subject.clone(),
                    version: r.version,
                    schema: r.text.clone(),
                })
                .collect(),
        };
        if rec.kind == SchemaType::Avro && !refs.is_empty() {
            let texts: Vec<&str> = refs.iter().map(|r| r.text.as_str()).collect();
            if let Ok(text) = crate::avro::inline(&rec.text, &texts) {
                out.schema = text;
                out.inlined = true;
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::{rfc3339, runs};

    #[test]
    fn times_are_utc_with_milliseconds_when_there_are_some() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(951_782_400_123), "2000-02-29T00:00:00.123Z");
        assert_eq!(rfc3339(951_782_400_120), "2000-02-29T00:00:00.12Z");
        assert_eq!(rfc3339(1_794_614_400_000), "2026-11-14T00:00:00Z");
    }

    #[test]
    fn ids_are_given_as_runs() {
        assert_eq!(runs([1, 2, 3, 5]), vec![[1, 3], [5, 5]]);
        assert!(runs([]).is_empty());
    }
}
