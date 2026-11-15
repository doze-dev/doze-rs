//! A response frame as the pieces it is written in.

use std::collections::VecDeque;
use std::io::IoSlice;

use bytes::{Buf, Bytes};

/// A response frame, ready for the socket: its length prefix, header and body, in order, in one or more pieces.
///
/// It is a [`Buf`], so a writer takes it as it is: `write_all_buf` hands a stream that writes vectored (a TCP socket,
/// rustls) many pieces in one call, and any other stream one piece at a time. A frame of one piece (every response but a
/// Fetch with records) allocates nothing beyond that piece.
#[derive(Debug, Clone, Default)]
pub struct Frame {
    /// The piece being written. Empty only when the whole frame has been.
    front: Bytes,
    /// The pieces after it.
    rest: VecDeque<Bytes>,
    /// The bytes left in all of them.
    remaining: usize,
}

impl Frame {
    /// Appends `piece` to the frame.
    pub(super) fn push(&mut self, piece: Bytes) {
        // An empty piece would only cost a slot in a vectored write (and break the rule that `front` is empty only at the end).
        if piece.is_empty() {
            return;
        }
        self.remaining += piece.len();
        if self.front.is_empty() {
            self.front = piece;
        } else {
            self.rest.push_back(piece);
        }
    }

    /// How many pieces are left to write.
    #[must_use]
    pub fn pieces(&self) -> usize {
        usize::from(!self.front.is_empty()) + self.rest.len()
    }
}

impl From<Bytes> for Frame {
    fn from(whole: Bytes) -> Self {
        let mut frame = Self::default();
        frame.push(whole);
        frame
    }
}

impl Buf for Frame {
    fn remaining(&self) -> usize {
        self.remaining
    }

    fn chunk(&self) -> &[u8] {
        &self.front
    }

    fn advance(&mut self, mut cnt: usize) {
        // Advancing past the end is a caller's bug; it stops at the end rather than panicking.
        while cnt > 0 && !self.front.is_empty() {
            let step = cnt.min(self.front.len());
            self.front.advance(step);
            self.remaining -= step;
            cnt -= step;
            if self.front.is_empty() {
                self.front = self.rest.pop_front().unwrap_or_default();
            }
        }
    }

    fn chunks_vectored<'a>(&'a self, dst: &mut [IoSlice<'a>]) -> usize {
        let pieces = std::iter::once(&self.front)
            .chain(&self.rest)
            .take_while(|p| !p.is_empty());
        let mut filled = 0;
        for (slot, piece) in dst.iter_mut().zip(pieces) {
            *slot = IoSlice::new(piece);
            filled += 1;
        }
        filled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(pieces: &[&'static [u8]]) -> Frame {
        let mut f = Frame::default();
        for p in pieces {
            f.push(Bytes::from_static(p));
        }
        f
    }

    #[test]
    fn empty_pieces_are_not_kept() {
        let f = frame(&[b"", b"ab", b"", b"c", b""]);
        assert_eq!((f.pieces(), f.remaining()), (2, 3));
        assert_eq!(Frame::from(Bytes::new()).pieces(), 0);
    }

    #[test]
    fn advancing_across_pieces_reads_the_bytes_in_order() {
        let mut f = frame(&[b"abc", b"de", b"fgh"]);
        let mut seen = Vec::new();
        // Steps that end inside a piece, on a boundary, and across one.
        for step in [2, 1, 3, 1, 5] {
            let take = step.min(f.remaining());
            let mut got = vec![0; take];
            f.copy_to_slice(&mut got);
            seen.extend(got);
        }
        assert_eq!(seen, b"abcdefgh");
        assert_eq!((f.remaining(), f.pieces(), f.chunk()), (0, 0, &b""[..]));
        // Past the end it stays at the end.
        f.advance(10);
        assert_eq!(f.remaining(), 0);
    }

    #[test]
    fn a_vectored_write_is_given_every_piece_that_fits() {
        let mut f = frame(&[b"abc", b"de", b"fgh"]);
        f.advance(1);
        let mut slots = [IoSlice::new(&[]); 2];
        assert_eq!(f.chunks_vectored(&mut slots), 2);
        assert_eq!((&*slots[0], &*slots[1]), (&b"bc"[..], &b"de"[..]));
        let mut slots = [IoSlice::new(&[]); 8];
        assert_eq!(f.chunks_vectored(&mut slots), 3);
        assert_eq!(&*slots[2], b"fgh");
    }
}
