//! Walking a run of batches (a segment's bytes) without collecting them.

use bytes::Bytes;

use super::{Batch, BatchError, layout};

/// The batches in a run of bytes, one at a time. Ends at the first fault, which it reports once.
#[derive(Debug, Clone)]
pub struct Batches {
    rest: Bytes,
    at: usize,
    failed: bool,
}

impl Batches {
    /// Walks `bytes`, which should be batches back to back.
    #[must_use]
    pub fn new(bytes: Bytes) -> Self {
        Self {
            rest: bytes,
            at: 0,
            failed: false,
        }
    }

    fn fail(&mut self, error: BatchError) -> Option<Result<Batch, BatchError>> {
        self.failed = true;
        Some(Err(error))
    }
}

impl Iterator for Batches {
    type Item = Result<Batch, BatchError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.failed || self.rest.is_empty() {
            return None;
        }
        let at = self.at;
        let Some(total) = layout::declared_total(&self.rest).filter(|t| *t <= self.rest.len())
        else {
            return self.fail(BatchError::Truncated(at));
        };
        let one = self.rest.split_to(total);
        self.at += total;
        match Batch::parse(one) {
            Ok(batch) => Some(Ok(batch)),
            Err(error) => self.fail(BatchError::InPayload {
                at,
                error: Box::new(error),
            }),
        }
    }
}
