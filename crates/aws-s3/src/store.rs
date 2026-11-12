//! The versions table: every object version and delete marker of every bucket, on disk and read in key order. A key's versions sit
//! together, newest first, because the row key ends in the inverse of the version's sequence number.

use aws_wire::ApiError;
use doze_store::record;

use crate::core::{Core, ROW_VERSION, VERSION, VERSIONS};
use crate::model::{Version, VersionV1};

/// The row key of a version.
pub(crate) fn row_key(bucket: &str, key: &str, seq: u64) -> String {
    format!("{bucket}\0{key}\0{:016x}", u64::MAX - seq)
}

/// The start of every row of a key.
pub(crate) fn key_prefix(bucket: &str, key: &str) -> String {
    format!("{bucket}\0{key}\0")
}

/// Reads a version row of either record version (1 was written before Object Lock and has no lock).
pub(crate) fn decode_version(bytes: &[u8]) -> Result<Version, doze_store::StoreError> {
    let (version, body) = record::split("version", bytes)?;
    match version {
        ROW_VERSION => record::body("version", body),
        VERSION => Ok(record::body::<VersionV1>("version", body)?.into()),
        other => Err(doze_store::StoreError::Corrupt(format!(
            "version: version {other} is not known"
        ))),
    }
}

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError::internal(format!("the object store failed: {e}"))
}

impl Core {
    /// The versions of `key`, newest first.
    pub(crate) fn versions_of(&self, bucket: &str, key: &str) -> Result<Vec<Version>, ApiError> {
        let prefix = key_prefix(bucket, key);
        let mut out = Vec::new();
        let mut bad = None;
        self.db
            .scan(VERSIONS, &prefix, |row, bytes| {
                if !row.starts_with(&prefix) {
                    return false;
                }
                match crate::store::decode_version(bytes) {
                    Ok(v) => out.push(v),
                    Err(e) => bad = Some(e),
                }
                bad.is_none()
            })
            .map_err(internal)?;
        bad.map_or(Ok(out), |e| Err(internal(e)))
    }

    /// The newest version of `key`, a delete marker if that is what it is.
    pub(crate) fn latest(&self, bucket: &str, key: &str) -> Result<Option<Version>, ApiError> {
        let prefix = key_prefix(bucket, key);
        let mut found = None;
        let mut bad = None;
        self.db
            .scan(VERSIONS, &prefix, |row, bytes| {
                if row.starts_with(&prefix) {
                    match crate::store::decode_version(bytes) {
                        Ok(v) => found = Some(v),
                        Err(e) => bad = Some(e),
                    }
                }
                false
            })
            .map_err(internal)?;
        bad.map_or(Ok(found), |e| Err(internal(e)))
    }

    /// The version of `key` called `id`.
    pub(crate) fn find(
        &self,
        bucket: &str,
        key: &str,
        id: &str,
    ) -> Result<Option<Version>, ApiError> {
        Ok(self
            .versions_of(bucket, key)?
            .into_iter()
            .find(|v| v.version_id == id))
    }

    /// Whether the bucket holds any version at all (a delete marker counts).
    pub(crate) fn has_objects(&self, bucket: &str) -> Result<bool, ApiError> {
        let prefix = format!("{bucket}\0");
        let mut any = false;
        self.db
            .scan(VERSIONS, &prefix, |row, _| {
                any = row.starts_with(&prefix);
                false
            })
            .map_err(internal)?;
        Ok(any)
    }

    /// Records a version to be written.
    pub(crate) fn put_row(&mut self, bucket: &str, v: &Version) -> Result<(), ApiError> {
        let row = row_key(bucket, &v.key, v.seq);
        let bytes = record::encode("version", ROW_VERSION, v).map_err(internal)?;
        self.batch.deletes.retain(|d| *d != row);
        self.batch.puts.retain(|(k, _)| *k != row);
        self.batch.puts.push((row, bytes));
        Ok(())
    }

    /// Records a version to be removed, and its blobs with it once the removal is on disk.
    pub(crate) fn drop_row(&mut self, bucket: &str, v: &Version) {
        let row = row_key(bucket, &v.key, v.seq);
        self.batch.puts.retain(|(k, _)| *k != row);
        self.batch.deletes.push(row);
        self.doomed.extend(v.segs.iter().map(|s| s.blob.clone()));
    }

    /// The next sequence number of `bucket`.
    pub(crate) fn next_seq(&mut self, bucket: &str) -> u64 {
        self.buckets.get_mut(bucket).map_or(0, |b| {
            b.next_seq += 1;
            b.next_seq
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::model::{Lock, Meta};

    fn version(lock: Lock) -> Version {
        Version {
            key: "k".into(),
            version_id: "null".into(),
            seq: 7,
            delete_marker: false,
            modified_ms: 1,
            size: 3,
            etag: "e".into(),
            meta: Meta::default(),
            checksum: None,
            segs: Vec::new(),
            parts: Vec::new(),
            tags: Vec::new(),
            lock,
        }
    }

    #[test]
    fn a_row_written_before_object_lock_is_read_without_a_lock_and_a_new_one_keeps_its_lock() {
        // Version 1 is version 2 without the lock at the end, which a version with no lock writes as two "none" bytes.
        let mut old = vec![VERSION];
        let new = record::encode("version", ROW_VERSION, &version(Lock::default())).unwrap();
        old.extend_from_slice(&new[1..new.len() - 2]);
        let read = decode_version(&old).unwrap();
        assert_eq!((read.seq, read.size, read.lock), (7, 3, Lock::default()));

        let held = Lock {
            retention: None,
            legal_hold: Some(true),
        };
        let bytes = record::encode("version", ROW_VERSION, &version(held)).unwrap();
        assert_eq!(decode_version(&bytes).unwrap().lock, held);
        assert!(
            decode_version(&[9, 0]).is_err(),
            "an unknown version is named, not guessed"
        );
    }
}
