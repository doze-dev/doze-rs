//! Making appends durable, and telling the appends waiting for it.
//!
//! A flush is the unit of durability, and one flush covers every append before it: a writer that has taken a
//! queue of appends syncs once, and every ticket among them is satisfied. A flush that fails is never
//! retried. The kernel may have thrown the unflushed pages away and reported the error once, so a second
//! flush returning success would acknowledge data that is gone: the partition goes offline instead, until a
//! restart decides from what is really on disk.

use std::sync::Arc;

use tokio::sync::watch;

use super::PartitionLog;
use super::shared::SyncState;
use crate::error::StorageError;
use crate::fs::Fs;

/// A claim on the durability of an append, or of everything appended so far.
#[derive(Debug)]
pub struct Ticket {
    seq: u64,
    sync: watch::Receiver<SyncState>,
}

impl Ticket {
    /// Whether the append is already on disk.
    #[must_use]
    pub fn is_durable(&self) -> bool {
        self.sync.borrow().synced >= self.seq
    }

    /// Waits until the append is on disk. Wakes with an error if a flush fails first.
    ///
    /// # Errors
    /// [`StorageError::Offline`] if a flush failed (or the log is gone) before this append was covered.
    pub async fn durable(mut self) -> Result<(), StorageError> {
        loop {
            let state = self.sync.borrow_and_update().clone();
            if state.synced >= self.seq {
                return Ok(());
            }
            if let Some(cause) = state.failed {
                return Err(StorageError::Offline {
                    cause: cause.to_string(),
                });
            }
            if self.sync.changed().await.is_err() {
                return Err(StorageError::Offline {
                    cause: "the log was closed before the append was flushed".into(),
                });
            }
        }
    }
}

impl<F: Fs> PartitionLog<F> {
    /// A claim on everything appended so far, including what was read back when the log was opened.
    #[must_use]
    pub fn ticket(&self) -> Ticket {
        self.ticket_for(self.append_seq)
    }

    pub(super) fn ticket_for(&self, seq: u64) -> Ticket {
        Ticket {
            seq,
            sync: self.shared.sync.subscribe(),
        }
    }

    /// Whether a failed flush has taken the partition's writes offline.
    #[must_use]
    pub fn is_offline(&self) -> bool {
        self.offline.is_some()
    }

    pub(super) fn check_online(&self) -> Result<(), StorageError> {
        match &self.offline {
            Some(cause) => Err(StorageError::Offline {
                cause: cause.to_string(),
            }),
            None => Ok(()),
        }
    }

    /// Takes the partition offline for `cause`, tells every ticket waiting, and returns the failure.
    pub(super) fn fail(&mut self, doing: &'static str, cause: std::io::Error) -> StorageError {
        let text: Arc<str> = format!("{doing}: {cause}").into();
        self.offline.get_or_insert_with(|| Arc::clone(&text));
        self.shared.sync.send_modify(|s| s.failed = Some(text));
        StorageError::Io {
            doing,
            source: cause,
        }
    }

    /// Flushes what is not known to be on disk: the active segment's log and index, and the directory if its
    /// entries changed. When it returns Ok, every append before the call (and everything read back when the log
    /// was opened) is durable. Nothing dirty means no system call.
    ///
    /// # Errors
    /// A failed flush, which takes the partition offline for good; or the offline error of an earlier one.
    pub fn sync(&mut self) -> Result<(), StorageError> {
        self.check_online()?;
        let upto = self.append_seq;
        if self.synced_seq >= upto && !self.dir_dirty && !self.active.is_dirty() {
            return Ok(());
        }
        self.flush_active()?;
        self.flush_directory()?;
        self.synced_seq = upto;
        self.shared.sync.send_modify(|s| s.synced = upto);
        Ok(())
    }

    /// Flushes the active segment; used by a sync and by the roll that seals it.
    pub(super) fn flush_active(&mut self) -> Result<(), StorageError> {
        match self.active.sync() {
            Ok(()) => Ok(()),
            Err(e) => Err(self.fail("flushing the active segment", e)),
        }
    }

    /// Flushes the directory's entries if they changed: until then a segment created, or removed, is so only
    /// in memory, and a power cut can take a fully flushed segment away with its name.
    pub(super) fn flush_directory(&mut self) -> Result<(), StorageError> {
        if !self.dir_dirty {
            return Ok(());
        }
        if let Err(e) = self.fs.sync_dir(&self.dir) {
            return Err(self.fail("flushing the partition directory", e));
        }
        self.dir_dirty = false;
        Ok(())
    }

    /// A graceful stop: flush everything. The log is not usable afterwards.
    ///
    /// # Errors
    /// A failed flush.
    pub fn close(mut self) -> Result<(), StorageError> {
        self.sync()
    }
}

impl<F: Fs> Drop for PartitionLog<F> {
    /// A ticket waits on a channel that outlives the log if a reader does. When the log goes, so does the
    /// chance that an append it has not flushed ever will be: say so, instead of leaving the ticket waiting.
    fn drop(&mut self) {
        self.shared.sync.send_modify(|s| {
            s.failed.get_or_insert_with(|| "the log was closed".into());
        });
    }
}
