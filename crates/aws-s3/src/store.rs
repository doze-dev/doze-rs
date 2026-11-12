//! The versions table: every object version and delete marker of every bucket, on disk and read in key order. A key's versions sit
//! together, newest first, because the row key ends in the inverse of the version's sequence number.

use aws_wire::ApiError;
use doze_store::record;

use crate::core::{Core, VERSION, VERSIONS};
use crate::model::Version;

/// The row key of a version.
pub(crate) fn row_key(bucket: &str, key: &str, seq: u64) -> String {
    format!("{bucket}\0{key}\0{:016x}", u64::MAX - seq)
}

/// The start of every row of a key.
pub(crate) fn key_prefix(bucket: &str, key: &str) -> String {
    format!("{bucket}\0{key}\0")
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
                match record::decode::<Version>("version", VERSION, bytes) {
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
                    match record::decode::<Version>("version", VERSION, bytes) {
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
        let bytes = record::encode("version", VERSION, v).map_err(internal)?;
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
