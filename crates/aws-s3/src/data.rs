//! A region's buckets as data (`doze-aws data`): what each bucket's objects, old versions, delete markers and unfinished uploads
//! hold, on disk; and removing some or all of it while the bucket and every configuration it has stay.
//!
//! Removal goes as the S3 API's does: the rows are committed first, then the blobs they pointed at are removed, then the file is
//! compacted; a crash in between leaves blobs nothing refers to, which the next start sweeps. No event notification is sent and no
//! lifecycle rule is consulted: this is a person clearing their disk, not a call AWS would see.
//!
//! **Object Lock gives way here.** A version under retention or a legal hold is removed like any other: the disk is the person's
//! own and nothing else can free it (a COMPLIANCE retention cannot be lifted through the API, by design). Usage says so in a note
//! before anything is removed, and a dry run counts those versions with the rest.

use std::collections::HashSet;

use aws_wire::storage::{Emptied, Prune, StorageError};
use doze_store::record;

use crate::core::{Core, VERSION, VERSIONS};
use crate::model::Version;

/// One row of the versions table with what it holds on disk.
pub(crate) struct Row {
    pub(crate) v: Version,
    /// The row's key and value and the blobs it points at.
    pub(crate) bytes: u64,
    /// The newest version of its key.
    pub(crate) current: bool,
}

/// An unfinished multipart upload with what it holds.
pub(crate) struct Pending {
    pub(crate) row: String,
    pub(crate) key: String,
    pub(crate) initiated_ms: i64,
    pub(crate) parts: u64,
    pub(crate) bytes: u64,
}

pub(crate) fn encoded<T: serde::Serialize>(what: &str, key: &str, value: &T) -> u64 {
    (key.len() + record::encode(what, VERSION, value).map_or(0, |b| b.len())) as u64
}

fn under(p: &Prune, key: &str) -> bool {
    p.prefix.as_deref().is_none_or(|pre| key.starts_with(pre))
}

/// Whether a prune removes this version: by kind (everything; old versions and delete markers; or, for `unused` alone, none), then
/// by prefix and time.
pub(crate) fn picks_row(p: &Prune, r: &Row) -> bool {
    let kind = if p.old_versions {
        !r.current || r.v.delete_marker
    } else {
        !p.unused
    };
    kind && under(p, &r.v.key) && p.before_ms.is_none_or(|b| r.v.modified_ms < b)
}

/// Whether a prune removes this upload: `unused` (uploads no object uses yet) or a prune of everything; not `old-versions` alone.
pub(crate) fn picks_upload(p: &Prune, u: &Pending) -> bool {
    (p.unused || !p.old_versions)
        && under(p, &u.key)
        && p.before_ms.is_none_or(|b| u.initiated_ms < b)
}

impl Core {
    /// Every version and delete marker of `bucket`, in key order, newest first within a key.
    pub(crate) fn rows_of(&self, bucket: &str) -> Result<Vec<Row>, String> {
        let prefix = format!("{bucket}\0");
        let mut out: Vec<Row> = Vec::new();
        let mut bad = None;
        self.db
            .scan(VERSIONS, &prefix, |row, bytes| {
                if !row.starts_with(&prefix) {
                    return false;
                }
                match crate::store::decode_version(bytes) {
                    Ok(v) => {
                        let current = out.last().is_none_or(|last| last.v.key != v.key);
                        let blobs: u64 = v.segs.iter().map(|s| s.len).sum();
                        let bytes = (row.len() + bytes.len()) as u64 + blobs;
                        out.push(Row { v, bytes, current });
                    }
                    Err(e) => bad = Some(e.to_string()),
                }
                bad.is_none()
            })
            .map_err(|e| e.to_string())?;
        bad.map_or(Ok(out), Err)
    }

    /// The unfinished multipart uploads of `bucket`: their rows, their parts' rows and the parts' blobs.
    pub(crate) fn pending_of(&self, bucket: &str) -> Vec<Pending> {
        self.uploads
            .with_prefix(&format!("{bucket}\0"))
            .map(|(row, u)| {
                let mut bytes = encoded("upload", row, u);
                bytes += self
                    .upload_locks
                    .get(row)
                    .map_or(0, |l| encoded("upload lock", row, l));
                bytes += self
                    .upload_acls
                    .get(row)
                    .map_or(0, |a| encoded("upload acl", row, a));
                let mut parts = 0;
                for (k, p) in self.parts.with_prefix(&format!("{}\0", u.id)) {
                    parts += 1;
                    bytes += encoded("part", k, p) + p.size;
                }
                Pending {
                    row: row.to_owned(),
                    key: u.key.clone(),
                    initiated_ms: u.initiated_ms,
                    parts,
                    bytes,
                }
            })
            .collect()
    }

    /// Removes what `prune` names from `bucket`; the bucket and its configuration stay.
    pub(crate) async fn empty_bucket(
        &mut self,
        name: &str,
        prune: &Prune,
    ) -> Result<Emptied, StorageError> {
        if self.buckets.get(name).is_none() {
            return Err(StorageError::NotFound);
        }
        let rows: Vec<Row> = self
            .rows_of(name)
            .map_err(StorageError::Failed)?
            .into_iter()
            .filter(|r| picks_row(prune, r))
            .collect();
        let pending: Vec<Pending> = self
            .pending_of(name)
            .into_iter()
            .filter(|u| picks_upload(prune, u))
            .collect();
        let mut done = Emptied::default();
        for bytes in rows
            .iter()
            .map(|r| r.bytes)
            .chain(pending.iter().map(|u| u.bytes))
        {
            done.items += 1;
            done.bytes += bytes;
        }
        if prune.dry_run || done.items == 0 {
            return Ok(done);
        }
        for r in &rows {
            self.drop_row(name, &r.v);
        }
        for u in &pending {
            self.remove_upload(&u.row);
        }
        done.freed = self.settle().await.map_err(StorageError::Failed)?;
        Ok(done)
    }

    /// Removes every bucket of the region, everything in them, and every blob.
    pub(crate) async fn reset(&mut self, dry_run: bool) -> Result<Emptied, String> {
        let names: Vec<String> = self.buckets.iter().map(|(n, _)| n.to_owned()).collect();
        let mut done = Emptied::default();
        let mut rows = Vec::new();
        for name in &names {
            let bucket_rows = self.rows_of(name)?;
            done.items += bucket_rows.len() as u64;
            done.bytes += bucket_rows.iter().map(|r| r.bytes).sum::<u64>();
            done.bytes += self.pending_of(name).iter().map(|u| u.bytes).sum::<u64>();
            done.bytes += self.bucket_row_bytes(name);
            rows.push((name.clone(), bucket_rows));
        }
        done.items += self.uploads.len() as u64;
        if dry_run {
            return Ok(done);
        }
        for (name, bucket_rows) in rows {
            for r in &bucket_rows {
                self.drop_row(&name, &r.v);
            }
            self.buckets.remove(&name);
        }
        self.forget_uploads();
        let freed = self.settle().await?;
        // Blobs a crash left behind with nothing pointing at them go too: after a reset nothing points at any.
        done.freed = freed + self.blobs.sweep(Some(&HashSet::new()));
        self.sync_marker();
        Ok(done)
    }

    /// Every upload and part, including any part whose upload is gone.
    fn forget_uploads(&mut self) {
        let rows: Vec<String> = self.uploads.iter().map(|(k, _)| k.to_owned()).collect();
        for row in rows {
            self.remove_upload(&row);
        }
        let strays: Vec<(String, String)> = self
            .parts
            .iter()
            .map(|(k, p)| (k.to_owned(), p.blob.clone()))
            .collect();
        for (k, blob) in strays {
            self.parts.remove(&k);
            self.doomed.push(blob);
        }
    }

    /// Writes what changed and waits for it to be on disk, then removes the blobs it no longer points at, then compacts the file.
    /// Answers the bytes the disk got back.
    pub(crate) async fn settle(&mut self) -> Result<u64, String> {
        let doomed = std::mem::take(&mut self.doomed);
        let saved = self.flush().map_err(|e| format!("could not save: {e}"))?;
        if let Some(ack) = saved {
            ack.await.map_err(|e| format!("could not save: {e}"))?;
        }
        let mut freed: u64 = doomed.iter().map(|b| self.blobs.remove_counted(b)).sum();
        freed += self
            .db
            .compact_off_loop()
            .await
            .map_err(|e| e.to_string())?
            .freed();
        Ok(freed)
    }

    /// Compacts the file; answers the bytes freed.
    pub(crate) async fn compact(&mut self) -> Result<u64, String> {
        self.db
            .compact_off_loop()
            .await
            .map(|c| c.freed())
            .map_err(|e| e.to_string())
    }
}
