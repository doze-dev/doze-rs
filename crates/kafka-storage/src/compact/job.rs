//! One round of cleaning a partition (source: `Cleaner.doClean`, `buildOffsetMap`, `cleanInto`): the latest offset of each
//! key in the dirty part is learned, then every segment up to there is gone through, and the records the map supersedes,
//! the deletions whose time has come and the records of aborted transactions are left out of a new file for the segment.
//! Nothing here changes the log: it leaves new files beside it, for the writer to put in place.

use std::path::Path;

use bytes::Bytes;
use kafka_wire::batch::{Batch, Marker, Rules};

use super::offset_map::OffsetMap;
use super::retain::retain;
use super::rewrite::Rewrite;
use super::source::{Found, Source};
use super::txn::CleanedTxns;
use super::types::{CleanJob, Cleaned, Input, Replacement, temporary};
use crate::error::{Doing, StorageError};
use crate::fs::Fs;
use crate::index::IndexWriter;
use crate::scan;

/// The most a batch may inflate to while it is cleaned.
const MOST_INFLATED: u64 = 256 << 20;

impl<F: Fs> CleanJob<F> {
    /// Does the round.
    ///
    /// # Errors
    /// A failed read or write, or a segment that is not made of batches. Files begun are taken back.
    pub fn run(self) -> Result<Cleaned, StorageError> {
        let map = self.offset_map()?;
        let first_dirty = self.plan.first_dirty;
        let mut done = Cleaned {
            clean_to: first_dirty,
            ..Cleaned::default()
        };
        if map.latest() < first_dirty {
            return Ok(done);
        }
        let clean_to = map.latest() + 1;
        done.clean_to = clean_to;
        let mut txns = CleanedTxns::new(self.aborted.clone());
        for input in self.segments.iter().filter(|s| s.base.get() < clean_to) {
            match self.clean_segment(input, &map, &mut txns, &mut done) {
                Ok(Some(r)) => done.replacements.push(r),
                Ok(None) => {}
                Err(e) => {
                    self.take_back(&done.replacements);
                    return Err(e);
                }
            }
        }
        Ok(done)
    }

    /// Takes back the files of replacements that will not be put in place.
    pub fn take_back(&self, replacements: &[Replacement]) {
        for r in replacements {
            let (log, index) = temporary(&self.dir, r.base);
            let _ = self.fs.remove_file(&log);
            let _ = self.fs.remove_file(&index);
        }
    }

    /// The latest offset of each key from the first dirty offset up to the limit, as far as the map has room.
    fn offset_map(&self) -> Result<OffsetMap, StorageError> {
        let (first_dirty, upto) = (self.plan.first_dirty, self.plan.upto);
        let mut map = OffsetMap::new(self.params.map_bytes);
        // Aborted records are not in the map: they are going, and must not take the place of a committed value before them
        // (source: `buildOffsetMapForSegment`).
        let mut txns = CleanedTxns::new(self.aborted.clone());
        'segments: for input in &self.segments {
            if input.base.get() >= upto || input.next.get() <= first_dirty {
                continue;
            }
            let from = if input.base.get() < first_dirty {
                input
                    .view
                    .seek(first_dirty)
                    .doing("finding where cleaning begins")?
            } else {
                0
            };
            let mut source = Source::new(&input.view, from);
            while let Some(found) = source.next().doing("reading a segment to clean it")? {
                let next = found.peek.next_offset().unwrap_or(i64::MAX);
                if next <= first_dirty {
                    continue;
                }
                if found.peek.base_offset >= upto || !self.map_batch(&mut map, &mut txns, &found)? {
                    break 'segments;
                }
                map.cover_to(next - 1);
            }
        }
        Ok(map)
    }

    /// Notes the keys of a batch, unless it is of an aborted transaction. `false` if the map is full.
    fn map_batch(
        &self,
        map: &mut OffsetMap,
        txns: &mut CleanedTxns,
        found: &Found,
    ) -> Result<bool, StorageError> {
        let peek = &found.peek;
        let last_offset = peek.next_offset().unwrap_or(i64::MAX) - 1;
        let producer = peek.producer.map_or(-1, |p| p.id.get());
        if peek.attributes.is_control() {
            let commit = Marker::read(&found.bytes).map(|m| m == Marker::Commit);
            txns.on_marker(producer, commit, last_offset);
            return Ok(true);
        }
        if txns.on_batch(producer, peek.attributes.is_transactional(), last_offset) {
            return Ok(true);
        }
        let batch = parse(&found.bytes)?;
        let mut room = true;
        batch
            .each_record(MOST_INFLATED, |seen| {
                if let (true, Some(key)) = (room, seen.key) {
                    room = map.put(key, seen.offset);
                }
            })
            .map_err(|e| unreadable(&e.reason))?;
        Ok(room)
    }

    /// One segment: its new files if cleaning changed it.
    fn clean_segment(
        &self,
        input: &Input<F>,
        map: &OffsetMap,
        txns: &mut CleanedTxns,
        done: &mut Cleaned,
    ) -> Result<Option<Replacement>, StorageError> {
        let (log_tmp, index_tmp) = temporary(&self.dir, input.base);
        let mut out = Rewrite::new(&self.fs, &input.view, log_tmp.clone());
        let mut source = Source::new(&input.view, 0);
        let size = source.size();
        let mut last_next = input.base.get();
        let walked = (|| -> Result<(), StorageError> {
            while let Some(found) = source.next().doing("reading a segment to clean it")? {
                last_next = found.peek.next_offset().unwrap_or(last_next);
                let last_in_segment = found.position + found.peek.total_len >= size;
                let fate = self.fate(&found, last_in_segment, map, txns, done)?;
                match fate {
                    Fate::Same => {}
                    Fate::Drop => out
                        .change(found.position, found.peek.total_len, None)
                        .doing("writing a cleaned segment")?,
                    Fate::Replace(bytes) => out
                        .change(found.position, found.peek.total_len, Some(&bytes))
                        .doing("writing a cleaned segment")?,
                }
            }
            Ok(())
        })();
        if let Err(e) = walked {
            out.abandon();
            return Err(e);
        }
        let Some(new_size) = out.finish(size).doing("writing a cleaned segment")? else {
            return Ok(None);
        };
        if let Err(e) = self.check_and_index(input, &log_tmp, &index_tmp, new_size, last_next) {
            let _ = self.fs.remove_file(&log_tmp);
            let _ = self.fs.remove_file(&index_tmp);
            return Err(e);
        }
        Ok(Some(Replacement {
            base: input.base,
            from_size: size,
        }))
    }

    /// What is to be done with a batch: left as it is, written changed, or left out.
    fn fate(
        &self,
        found: &Found,
        last_in_segment: bool,
        map: &OffsetMap,
        txns: &mut CleanedTxns,
        done: &mut Cleaned,
    ) -> Result<Fate, StorageError> {
        let peek = &found.peek;
        let control = peek.attributes.is_control();
        let last_offset = peek.next_offset().unwrap_or(i64::MAX) - 1;
        let producer = peek.producer.map_or(-1, |p| p.id.get());
        let can_discard = if control {
            let commit = Marker::read(&found.bytes).map(|m| m == Marker::Commit);
            txns.on_marker(producer, commit, last_offset)
        } else {
            txns.on_batch(producer, peek.attributes.is_transactional(), last_offset)
        };
        let batch = parse(&found.bytes)?;
        let now = self.params.now_ms;
        let horizon = batch.delete_horizon();
        let discard_records = if control {
            can_discard && horizon.is_some_and(|h| h <= now)
        } else {
            can_discard
        };
        let retain_empty = last_in_segment || self.is_a_producers_last(found, control);
        let rules = Rules {
            max_inflated: MOST_INFLATED,
            horizon: now.saturating_add(self.params.delete_retention_ms),
            marker_of_empty_transaction: can_discard && control,
        };
        let filtered = batch
            .filter(rules, |seen| {
                !discard_records && (control || retain(map, horizon, now, seen))
            })
            .map_err(|e| unreadable(&e.reason))?;
        done.records_read += u64::from(peek.record_count);
        done.records_kept += u64::from(filtered.kept);
        Ok(if filtered.kept == 0 && !retain_empty {
            Fate::Drop
        } else if filtered.changed {
            Fate::Replace(filtered.batch.into_bytes())
        } else {
            Fate::Same
        })
    }

    fn is_a_producers_last(&self, found: &Found, control: bool) -> bool {
        let Some(p) = found.peek.producer else {
            return false;
        };
        let Some(last) = self.producers.get(&p.id.get()) else {
            return false;
        };
        match last.data_last_offset {
            Some(offset) => found.peek.next_offset().map(|n| n - 1) == Some(offset),
            None => control && p.epoch.get() == last.marker_epoch,
        }
    }

    /// Checks the new log is made of whole batches and ends where the old one did, and writes its index.
    fn check_and_index(
        &self,
        input: &Input<F>,
        log_tmp: &Path,
        index_tmp: &Path,
        new_size: u64,
        old_next: i64,
    ) -> Result<(), StorageError> {
        let file = self
            .fs
            .open_existing(log_tmp)
            .doing("checking a cleaned segment")?;
        let scanned = scan::scan(&file, new_size, input.base, self.index_interval, 0)
            .doing("checking a cleaned segment")?;
        if scanned.valid_end != new_size || scanned.next_offset.get() != old_next {
            return Err(unreadable(
                "a cleaned segment is not whole or ends elsewhere",
            ));
        }
        let (index, _) = self
            .fs
            .open_or_create(index_tmp)
            .doing("writing an index")?;
        let mut writer = IndexWriter::<F>::new(index).doing("writing an index")?;
        writer.rewrite(&scanned.index).doing("writing an index")?;
        writer.sync().doing("writing an index")
    }
}

enum Fate {
    Same,
    Drop,
    Replace(Bytes),
}

fn parse(bytes: &Bytes) -> Result<Batch, StorageError> {
    Batch::parse_stored(bytes.clone()).map_err(|e| unreadable(&e.to_string()))
}

fn unreadable(why: &str) -> StorageError {
    StorageError::Io {
        doing: "cleaning a segment",
        source: std::io::Error::new(std::io::ErrorKind::InvalidData, why.to_owned()),
    }
}
