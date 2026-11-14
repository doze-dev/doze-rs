//! Deleting: a version or a whole subject, soft first and permanent only after that. A version another schema
//! references cannot go; a schema no version holds any more goes with its last permanent version.

use super::{Mode, Registry, State};
use crate::error::{RegistryError, code};
use crate::store::{Batch, Key};

impl State {
    /// The ids no version would hold once the versions in `removing` (subject, number) are gone for good.
    fn orphans(&self, removing: &[(&str, u32)]) -> Vec<u32> {
        let gone = |name: &str, n: u32| removing.iter().any(|&(s, m)| s == name && m == n);
        let mut candidates: Vec<u32> = removing
            .iter()
            .filter_map(|&(s, n)| self.find(s, n).map(|v| v.rec.id))
            .collect();
        candidates.sort_unstable();
        candidates.dedup();
        candidates.retain(|id| {
            !self
                .subjects
                .iter()
                .any(|(name, vs)| vs.iter().any(|v| v.rec.id == *id && !gone(name, v.number)))
        });
        candidates
    }

    fn drop_schemas(&mut self, ids: &[u32]) {
        for id in ids {
            self.schemas.remove(id);
        }
        self.by_print.retain(|_, id| !ids.contains(id));
    }
}

impl Registry {
    /// Deletes one version: soft, or `permanent` once it is soft-deleted. Returns its number.
    ///
    /// # Errors
    /// A read-only subject, an invalid or missing version, the wrong order of soft and permanent, a version another
    /// schema references, a store failure.
    pub fn delete_version(
        &self,
        subject: &str,
        version: &str,
        permanent: bool,
    ) -> Result<u32, RegistryError> {
        let mut s = self.lock();
        if s.mode_of(subject) == Mode::ReadOnly {
            return Err(RegistryError::read_only(subject));
        }
        let v = s.version_of(subject, version, true)?.clone();
        let n = v.number;
        if permanent && !v.rec.deleted {
            return Err(RegistryError::new(
                404,
                code::VERSION_NOT_SOFT_DELETED,
                format!(
                    "Subject '{subject}' Version {n} was not deleted first before being permanently deleted"
                ),
            ));
        }
        if !permanent && v.rec.deleted {
            return Err(RegistryError::new(
                404,
                code::VERSION_SOFT_DELETED,
                format!(
                    "Subject '{subject}' Version {n} was soft deleted.Set permanent=true to delete permanently"
                ),
            ));
        }
        if !s.referencing(subject, n).is_empty() {
            return Err(RegistryError::reference_exists(subject, n));
        }
        let key = Key::Version(subject.to_owned(), n);
        let mut b = Batch::default();
        let gone = if permanent {
            s.orphans(&[(subject, n)])
        } else {
            Vec::new()
        };
        if permanent {
            b.delete(&key);
        } else {
            let mut rec = v.rec.clone();
            rec.deleted = true;
            b.put(&key, &rec);
        }
        for &id in &gone {
            b.delete(&Key::Schema(id));
        }
        self.commit(&b)?;
        if let Some(vs) = s.subjects.get_mut(subject) {
            if permanent {
                vs.retain(|x| x.number != n);
            } else if let Some(x) = vs.iter_mut().find(|x| x.number == n) {
                x.rec.deleted = true;
            }
            if vs.is_empty() {
                s.subjects.remove(subject);
            }
        }
        s.drop_schemas(&gone);
        Ok(n)
    }

    /// Deletes every version of a subject: soft, or `permanent` once they all are. Returns their numbers. The
    /// subject's own level and mode go with it, on the soft delete already (recorded: permanent-delete-rules).
    ///
    /// # Errors
    /// A read-only subject, no such subject, the wrong order of soft and permanent, a version another schema
    /// references, a store failure.
    pub fn delete_subject(
        &self,
        subject: &str,
        permanent: bool,
    ) -> Result<Vec<u32>, RegistryError> {
        let mut s = self.lock();
        if s.mode_of(subject) == Mode::ReadOnly {
            return Err(RegistryError::read_only(subject));
        }
        let has_any = !s.versions(subject).is_empty();
        let alive = s.exists(subject);
        if !has_any || (!permanent && !alive) {
            if has_any {
                return Err(RegistryError::new(
                    404,
                    code::SUBJECT_SOFT_DELETED,
                    format!(
                        "Subject '{subject}' was soft deleted.Set permanent=true to delete permanently"
                    ),
                ));
            }
            return Err(RegistryError::subject_not_found(subject));
        }
        if permanent && alive {
            return Err(RegistryError::new(
                404,
                code::SUBJECT_NOT_SOFT_DELETED,
                format!(
                    "Subject '{subject}' was not deleted first before being permanently deleted"
                ),
            ));
        }
        let targets: Vec<u32> = s
            .versions(subject)
            .iter()
            .filter(|v| permanent || !v.rec.deleted)
            .map(|v| v.number)
            .collect();
        if let Some(&n) = targets
            .iter()
            .find(|&&n| !s.referencing(subject, n).is_empty())
        {
            return Err(RegistryError::reference_exists(subject, n));
        }
        let mut b = Batch::default();
        for v in s
            .versions(subject)
            .iter()
            .filter(|v| targets.contains(&v.number))
        {
            let key = Key::Version(subject.to_owned(), v.number);
            if permanent {
                b.delete(&key);
            } else {
                let mut rec = v.rec.clone();
                rec.deleted = true;
                b.put(&key, &rec);
            }
        }
        let removing: Vec<(&str, u32)> = if permanent {
            targets.iter().map(|&n| (subject, n)).collect()
        } else {
            Vec::new()
        };
        let gone = s.orphans(&removing);
        for &id in &gone {
            b.delete(&Key::Schema(id));
        }
        b.delete(&Key::Level(subject.to_owned()));
        b.delete(&Key::Mode(subject.to_owned()));
        self.commit(&b)?;
        s.levels.remove(subject);
        s.modes.remove(subject);
        if permanent {
            s.subjects.remove(subject);
            s.drop_schemas(&gone);
        } else if let Some(vs) = s.subjects.get_mut(subject) {
            vs.iter_mut().for_each(|v| v.rec.deleted = true);
        }
        Ok(targets)
    }
}
