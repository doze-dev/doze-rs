//! The records section as the scan reads it: a byte at a time for the numbers, whole fields skipped.

use std::io::{self, BufRead};

/// Why reading a field stopped.
pub(super) enum Stop {
    /// The records section ended, cleanly, where a field should be.
    Cut,
    /// A number longer than a number can be, or a length that cannot be.
    Number,
    /// The source failed: a codec's data that is not its own, or the limit.
    Source(io::Error),
}

impl From<io::Error> for Stop {
    fn from(e: io::Error) -> Self {
        Self::Source(e)
    }
}

/// Why a record could not be read.
pub(super) enum Fault {
    /// It is not what a record is, or what its batch says.
    Malformed(String),
    /// The source failed.
    Source(io::Error),
}

/// The records section as the scan reads it, counting what it has taken.
pub(super) struct Cursor<R> {
    pub(super) source: R,
    pub(super) taken: u64,
}

impl<R: BufRead> Cursor<R> {
    pub(super) fn byte(&mut self) -> Result<u8, Stop> {
        let buffered = self.source.fill_buf()?;
        let &byte = buffered.first().ok_or(Stop::Cut)?;
        self.source.consume(1);
        self.taken += 1;
        Ok(byte)
    }

    /// A zigzag varint of at most 64 bits.
    pub(super) fn varint(&mut self) -> Result<i64, Stop> {
        let mut value = 0u64;
        for shift in (0..64).step_by(7) {
            let byte = self.byte()?;
            // The tenth byte may carry only the one bit that is left.
            if shift == 63 && byte > 1 {
                return Err(Stop::Number);
            }
            value |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Ok(((value >> 1) as i64) ^ -((value & 1) as i64));
            }
        }
        Err(Stop::Number)
    }

    pub(super) fn skip(&mut self, mut n: u64) -> Result<(), Stop> {
        while n > 0 {
            let buffered = self.source.fill_buf()?;
            if buffered.is_empty() {
                return Err(Stop::Cut);
            }
            let step = (buffered.len() as u64).min(n);
            self.source.consume(step as usize);
            self.taken += step;
            n -= step;
        }
        Ok(())
    }
}
