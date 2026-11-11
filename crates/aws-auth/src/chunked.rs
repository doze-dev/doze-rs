//! The aws-chunked content encoding, decoded and verified as it arrives: S3 uploads signed chunk by chunk
//! (`STREAMING-AWS4-HMAC-SHA256-PAYLOAD`), with a signed trailer, or unsigned with a trailing checksum. Sans-IO: push
//! `Bytes`, get zero-copy slices of the payload back, whatever the network framing.

use bytes::Bytes;
use sha2::{Digest, Sha256};

use crate::key::{EMPTY_SHA256, SigningKey, sha256_hex};
use aws_core::hex;

/// Longest chunk-header or trailer line we will buffer.
const MAX_LINE: usize = 4096;
#[derive(Debug, thiserror::Error)]
pub enum ChunkError {
    #[error("chunk header or trailer line longer than {MAX_LINE} bytes")]
    LineTooLong,
    #[error("chunk header is not valid: {0}")]
    BadHeader(&'static str),
    #[error("chunk payload not followed by CRLF")]
    BadTerminator,
    #[error("chunk signature missing")]
    MissingSignature,
    #[error("chunk signature does not match")]
    BadSignature,
    #[error("trailer signature missing or wrong")]
    BadTrailerSignature,
    #[error("stream ended inside the chunk framing")]
    Truncated,
}

/// Signing context for chunk-by-chunk verification.
#[derive(Debug)]
pub struct ChunkAuth {
    pub key: SigningKey,
    pub amz_date: String,
    pub scope: String,
    /// Seed signature (the request signature), then the previous chunk's.
    pub prev_signature: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Header,
    Data,
    DataCrlf,
    Trailer,
    Done,
}

/// Push-style decoder for the aws-chunked content encoding. Feed it a `Bytes`;
/// it hands back zero-copy slices of the payload.
#[derive(Debug)]
pub struct ChunkDecoder {
    state: State,
    line: Vec<u8>,
    remaining: u64,
    crlf_seen: u8,
    auth: Option<ChunkAuth>,
    signed_trailer: bool,
    chunk_hash: Sha256,
    declared_sig: Option<String>,
    trailer_text: String,
    trailer_sig: Option<String>,
    trailers: Vec<(String, String)>,
    seen_first_header: bool,
}

/// What [`ChunkDecoder::next`] found.
#[derive(Debug)]
pub enum Event {
    /// Payload bytes (a slice of the input).
    Data(Bytes),
    /// Input exhausted mid-frame.
    Need,
    /// Final chunk and trailers consumed and verified.
    Done,
}

impl ChunkDecoder {
    #[must_use]
    pub fn new(auth: Option<ChunkAuth>, signed_trailer: bool) -> Self {
        Self {
            state: State::Header,
            line: Vec::new(),
            remaining: 0,
            crlf_seen: 0,
            auth,
            signed_trailer,
            chunk_hash: Sha256::new(),
            declared_sig: None,
            trailer_text: String::new(),
            trailer_sig: None,
            trailers: Vec::new(),
            seen_first_header: false,
        }
    }

    /// Trailing headers (`x-amz-checksum-crc32`, ...), lower-cased names.
    #[must_use]
    pub fn trailers(&self) -> &[(String, String)] {
        &self.trailers
    }

    // fuzz:parser fuzz_chunked
    /// Advance as far as `input` allows, consuming what it uses.
    ///
    /// # Errors
    /// Framing and signature errors; after an error the decoder must be dropped.
    pub fn next(&mut self, input: &mut Bytes) -> Result<Event, ChunkError> {
        loop {
            match self.state {
                State::Done => return Ok(Event::Done),
                State::Data => {
                    if input.is_empty() {
                        return Ok(Event::Need);
                    }
                    let take =
                        usize::try_from(self.remaining).map_or(input.len(), |r| r.min(input.len()));
                    let data = input.split_to(take);
                    self.remaining -= data.len() as u64;
                    if self.auth.is_some() {
                        self.chunk_hash.update(&data);
                    }
                    if self.remaining == 0 {
                        self.state = State::DataCrlf;
                        self.crlf_seen = 0;
                    }
                    return Ok(Event::Data(data));
                }
                State::DataCrlf => {
                    while self.crlf_seen < 2 {
                        if input.is_empty() {
                            return Ok(Event::Need);
                        }
                        let b = input.split_to(1);
                        let expect = if self.crlf_seen == 0 { b'\r' } else { b'\n' };
                        if b.first() != Some(&expect) {
                            return Err(ChunkError::BadTerminator);
                        }
                        self.crlf_seen += 1;
                    }
                    self.verify_chunk()?;
                    self.state = State::Header;
                }
                State::Header | State::Trailer => {
                    if !self.read_line(input)? {
                        return Ok(Event::Need);
                    }
                    if self.state == State::Header {
                        self.on_header_line()?;
                    } else {
                        self.on_trailer_line()?;
                    }
                }
            }
        }
    }

    /// Called at end of input: a stream that stopped at the trailer boundary of an
    /// unsigned stream is accepted (some clients omit the closing blank line).
    ///
    /// # Errors
    /// [`ChunkError::Truncated`] anywhere else.
    pub fn finish_at_eof(&mut self) -> Result<(), ChunkError> {
        if self.state == State::Done {
            return Ok(());
        }
        if self.state == State::Trailer && self.line.is_empty() && self.auth.is_none() {
            self.state = State::Done;
            return Ok(());
        }
        Err(ChunkError::Truncated)
    }

    /// Accumulate bytes up to and including `\n` into `self.line`. `true` when a full line is ready.
    fn read_line(&mut self, input: &mut Bytes) -> Result<bool, ChunkError> {
        let (take, complete) = match input.iter().position(|b| *b == b'\n') {
            Some(p) => (p + 1, true),
            None => (input.len(), false),
        };
        let piece = input.split_to(take);
        if self.line.len() + piece.len() > MAX_LINE {
            return Err(ChunkError::LineTooLong);
        }
        self.line.extend_from_slice(&piece);
        Ok(complete)
    }

    fn take_line(&mut self) -> Result<String, ChunkError> {
        let raw = std::mem::take(&mut self.line);
        let s = String::from_utf8(raw).map_err(|_| ChunkError::BadHeader("not UTF-8"))?;
        Ok(s.trim_end_matches(['\r', '\n']).to_owned())
    }

    fn on_header_line(&mut self) -> Result<(), ChunkError> {
        let line = self.take_line()?;
        // Some clients emit a blank line before the first chunk; tolerate it once.
        if line.is_empty() && !self.seen_first_header {
            self.seen_first_header = true;
            return Ok(());
        }
        self.seen_first_header = true;
        let (size_hex, ext) = line.split_once(';').unwrap_or((line.as_str(), ""));
        let size = u64::from_str_radix(size_hex.trim(), 16)
            .map_err(|_| ChunkError::BadHeader("chunk size is not hex"))?;
        self.declared_sig = ext
            .split(';')
            .find_map(|e| e.trim().strip_prefix("chunk-signature="))
            .map(str::to_owned);
        self.chunk_hash = Sha256::new();
        if size == 0 {
            self.verify_chunk()?;
            self.state = State::Trailer;
        } else {
            self.remaining = size;
            self.state = State::Data;
        }
        Ok(())
    }

    fn on_trailer_line(&mut self) -> Result<(), ChunkError> {
        let line = self.take_line()?;
        if line.is_empty() {
            self.verify_trailer()?;
            self.state = State::Done;
            return Ok(());
        }
        let (name, value) = line
            .split_once(':')
            .ok_or(ChunkError::BadHeader("trailer line without ':'"))?;
        let (name, value) = (name.trim().to_ascii_lowercase(), value.trim().to_owned());
        if name == "x-amz-trailer-signature" {
            self.trailer_sig = Some(value);
        } else {
            self.trailer_text.push_str(&name);
            self.trailer_text.push(':');
            self.trailer_text.push_str(&value);
            self.trailer_text.push('\n');
            self.trailers.push((name, value));
        }
        Ok(())
    }

    /// Verify the signature of the chunk just completed (the hash covers its payload).
    fn verify_chunk(&mut self) -> Result<(), ChunkError> {
        let Some(auth) = self.auth.as_mut() else {
            return Ok(());
        };
        let declared = self
            .declared_sig
            .take()
            .ok_or(ChunkError::MissingSignature)?;
        let data_hash = hex::encode(&self.chunk_hash.finalize_reset());
        let sts = format!(
            "AWS4-HMAC-SHA256-PAYLOAD\n{}\n{}\n{}\n{EMPTY_SHA256}\n{data_hash}",
            auth.amz_date, auth.scope, auth.prev_signature
        );
        if !auth.key.verify_hex(sts.as_bytes(), &declared) {
            return Err(ChunkError::BadSignature);
        }
        auth.prev_signature = declared;
        Ok(())
    }

    fn verify_trailer(&mut self) -> Result<(), ChunkError> {
        let Some(auth) = self.auth.as_ref() else {
            return Ok(());
        };
        if !self.signed_trailer {
            return Ok(());
        }
        let claimed = self
            .trailer_sig
            .as_deref()
            .ok_or(ChunkError::BadTrailerSignature)?;
        let sts = format!(
            "AWS4-HMAC-SHA256-TRAILER\n{}\n{}\n{}\n{}",
            auth.amz_date,
            auth.scope,
            auth.prev_signature,
            sha256_hex(self.trailer_text.as_bytes())
        );
        if auth.key.verify_hex(sts.as_bytes(), claimed) {
            Ok(())
        } else {
            Err(ChunkError::BadTrailerSignature)
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]
    use super::*;

    fn drain(dec: &mut ChunkDecoder, pieces: &[&[u8]]) -> Result<Vec<u8>, ChunkError> {
        let mut out = Vec::new();
        for p in pieces {
            let mut input = Bytes::copy_from_slice(p);
            loop {
                match dec.next(&mut input)? {
                    Event::Data(d) => out.extend_from_slice(&d),
                    Event::Need => break,
                    Event::Done => return Ok(out),
                }
            }
        }
        dec.finish_at_eof()?;
        Ok(out)
    }

    #[test]
    fn unsigned_with_trailer_split_at_every_byte() {
        let wire = b"5\r\nhello\r\n6\r\n world\r\n0\r\nx-amz-checksum-crc32:AAAAAA==\r\n\r\n";
        // Feed one byte at a time: framing must survive any split.
        let pieces: Vec<&[u8]> = wire.chunks(1).collect();
        let mut dec = ChunkDecoder::new(None, false);
        let got = drain(&mut dec, &pieces).unwrap();
        assert_eq!(got, b"hello world");
        assert_eq!(
            dec.trailers(),
            &[("x-amz-checksum-crc32".to_owned(), "AAAAAA==".to_owned())]
        );
    }

    #[test]
    fn bad_terminator_and_truncation() {
        let mut dec = ChunkDecoder::new(None, false);
        assert!(matches!(
            drain(&mut dec, &[b"3\r\nabcXX"]),
            Err(ChunkError::BadTerminator)
        ));
        let mut dec = ChunkDecoder::new(None, false);
        assert!(matches!(
            drain(&mut dec, &[b"a\r\nabc"]),
            Err(ChunkError::Truncated)
        ));
        let mut dec = ChunkDecoder::new(None, false);
        assert!(matches!(
            drain(&mut dec, &[b"zz\r\n"]),
            Err(ChunkError::BadHeader(_))
        ));
        let mut dec = ChunkDecoder::new(None, false);
        let long = vec![b'1'; MAX_LINE + 8];
        assert!(matches!(
            drain(&mut dec, &[&long]),
            Err(ChunkError::LineTooLong)
        ));
    }
}
