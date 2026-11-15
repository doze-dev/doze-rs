//! What each bucket of a region holds on disk, counted from its rows and blobs: current objects, old versions, delete markers and
//! unfinished multipart uploads apart, with a note for each part a person would not know is there.

use aws_wire::storage::{File, Note, Prune, Resource};

use crate::core::Core;
use crate::data::encoded;
use crate::model::{Bucket, Version, Versioning};

/// A count and the bytes it holds.
#[derive(Clone, Copy, Default)]
struct Tally {
    n: u64,
    bytes: u64,
}

impl Tally {
    fn add(&mut self, bytes: u64) {
        self.n += 1;
        self.bytes += bytes;
    }
}

/// A bucket's data by kind.
#[derive(Default)]
struct Counts {
    current: Tally,
    old: Tally,
    markers: Tally,
    /// Versions Object Lock holds now (of any kind above).
    locked: Tally,
    uploads: Tally,
    parts: u64,
}

/// `1,234 old versions`.
pub(crate) fn counted(n: u64, one: &str, many: &str) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    format!("{out} {}", if n == 1 { one } else { many })
}

/// Whether Object Lock holds a version now.
fn held(v: &Version, now_ms: i64) -> bool {
    v.lock.legal_hold == Some(true) || v.lock.retention.is_some_and(|r| r.until_ms > now_ms)
}

/// Whether the bucket has an enabled lifecycle rule that expires noncurrent versions.
fn expires_old_versions(bucket: &Bucket) -> bool {
    bucket.configs.get("lifecycle").is_some_and(|c| {
        crate::lifecycle::rules_of(c)
            .iter()
            .any(|r| r.noncurrent_days.is_some())
    })
}

/// The notes of a bucket: old versions, unfinished uploads, what Object Lock holds, and versioning with nothing to expire it.
fn notes(c: &Counts, bucket: &Bucket) -> Vec<Note> {
    let mut notes = Vec::new();
    if c.old.n + c.markers.n > 0 {
        notes.push(Note {
            bytes: c.old.bytes + c.markers.bytes,
            text: format!(
                "in {} and {}: listed only by ListObjectVersions",
                counted(c.old.n, "old version", "old versions"),
                counted(c.markers.n, "delete marker", "delete markers")
            ),
            prune: Some(Prune {
                old_versions: true,
                ..Prune::default()
            }),
        });
    }
    if c.uploads.n > 0 {
        let uploads = counted(
            c.uploads.n,
            "unfinished multipart upload",
            "unfinished multipart uploads",
        );
        notes.push(Note {
            bytes: c.uploads.bytes,
            text: format!(
                "in {uploads} ({}): no object uses them until they are completed or aborted",
                counted(c.parts, "part", "parts")
            ),
            prune: Some(Prune {
                unused: true,
                ..Prune::default()
            }),
        });
    }
    if c.locked.n > 0 {
        notes.push(Note {
            bytes: c.locked.bytes,
            text: format!(
                "in {} under Object Lock retention or a legal hold: emptying here removes them anyway (AWS would refuse; this is your disk)",
                counted(c.locked.n, "version", "versions")
            ),
            prune: None,
        });
    }
    if bucket.versioning == Versioning::Enabled && !expires_old_versions(bucket) {
        notes.push(Note {
            bytes: 0,
            text: "versioning is on and no lifecycle rule expires old versions: every overwrite and delete keeps the old bytes"
                .to_owned(),
            prune: None,
        });
    }
    notes
}

impl Core {
    /// Each bucket of the region with what it holds.
    pub(crate) fn usage(&self, region: &str) -> Result<Vec<Resource>, String> {
        let mut out = Vec::new();
        for (name, bucket) in self.buckets.iter() {
            let c = self.counts(name)?;
            out.push(Resource {
                region: region.to_owned(),
                name: name.to_owned(),
                bytes: c.current.bytes + c.old.bytes + c.markers.bytes + c.uploads.bytes,
                items: c.current.n,
                notes: notes(&c, bucket),
            });
        }
        Ok(out)
    }

    fn counts(&self, name: &str) -> Result<Counts, String> {
        let now = self.now_ms();
        let mut c = Counts::default();
        for r in self.rows_of(name)? {
            match (r.v.delete_marker, r.current) {
                (true, _) => c.markers.add(r.bytes),
                (false, true) => c.current.add(r.bytes),
                (false, false) => c.old.add(r.bytes),
            }
            if held(&r.v, now) {
                c.locked.add(r.bytes);
            }
        }
        for p in self.pending_of(name) {
            c.uploads.add(p.bytes);
            c.parts += p.parts;
        }
        Ok(c)
    }

    /// The database file and what compacting it would give back.
    pub(crate) fn file(&self, region: &str) -> File {
        let space = self.db.space().unwrap_or_default();
        File {
            region: region.to_owned(),
            bytes: space.file,
            reclaimable: space.reclaimable(),
        }
    }

    /// The bytes a bucket's own row holds (its settings), for a reset's count.
    pub(crate) fn bucket_row_bytes(&self, name: &str) -> u64 {
        self.buckets
            .get(name)
            .map_or(0, |b| encoded("bucket", name, b))
    }
}
