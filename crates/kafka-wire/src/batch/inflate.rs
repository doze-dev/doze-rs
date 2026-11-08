//! Opening the records of a compressed batch as a stream, held to a limit.
//!
//! A compressed payload can claim far more than it weighs, so nothing here ever holds the inflated bytes: a stream
//! hands them out as they are read and fails the moment more than the limit has come out. What is held is the
//! decompressor's window and buffers. Each codec is a feature; a broker built without one refuses the batches that
//! use it.

use std::io::{self, Read};

use super::Compression;

/// Why a records section could not be opened or read.
#[derive(Debug)]
pub(super) enum InflateError {
    /// More than the limit came out, or a zstd frame asks for a window past what is allowed.
    TooLarge,
    /// This build has no decoder for the codec.
    Unsupported(Compression),
    /// The bytes are not what the codec says.
    Corrupt(String),
}

/// Marks the error a [`Bounded`] reader raises at its limit, so that it can be told from a codec's own.
#[derive(Debug)]
struct Exceeded;

impl std::fmt::Display for Exceeded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("more was inflated than is allowed")
    }
}

impl std::error::Error for Exceeded {}

/// What a read error means.
pub(super) fn why(e: &io::Error) -> InflateError {
    if e.get_ref().is_some_and(|inner| inner.is::<Exceeded>()) {
        InflateError::TooLarge
    } else {
        InflateError::Corrupt(e.to_string())
    }
}

/// A reader that fails, rather than ends, once `left` bytes have come out of it.
struct Bounded<R> {
    inner: R,
    left: u64,
}

impl<R: Read> Read for Bounded<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let got = self.inner.read(buf)?;
        let got_u64 = got as u64;
        if got_u64 > self.left {
            return Err(io::Error::other(Exceeded));
        }
        self.left -= got_u64;
        Ok(got)
    }
}

/// The records section `body` of a batch in `codec`, as a stream that fails past `limit` inflated bytes.
pub(super) fn open<'a>(
    codec: Compression,
    body: &'a [u8],
    limit: u64,
) -> Result<Box<dyn Read + 'a>, InflateError> {
    let stream: Box<dyn Read + 'a> = match codec {
        Compression::None => Box::new(body),
        #[cfg(feature = "gzip")]
        Compression::Gzip => Box::new(GzipMembers {
            member: Some(flate2::bufread::GzDecoder::new(body)),
        }),
        #[cfg(feature = "snappy")]
        Compression::Snappy => snappy(body, limit)?,
        #[cfg(feature = "lz4")]
        Compression::Lz4 => Box::new(lz4_flex::frame::FrameDecoder::new(body)),
        #[cfg(feature = "zstd")]
        Compression::Zstd => zstd(body, limit)?,
        other => return Err(InflateError::Unsupported(other)),
    };
    Ok(Box::new(Bounded {
        inner: stream,
        left: limit,
    }))
}

/// The members of a gzip stream, one after another: when a member ends, another follows only if the next bytes begin
/// one, and anything else after the last member is not read (Kafka stops there too, and so a batch with bytes after its
/// stream is accepted).
#[cfg(feature = "gzip")]
struct GzipMembers<'a> {
    member: Option<flate2::bufread::GzDecoder<&'a [u8]>>,
}

#[cfg(feature = "gzip")]
impl Read for GzipMembers<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            let Some(member) = self.member.as_mut() else {
                return Ok(0);
            };
            let got = member.read(buf)?;
            if got > 0 || buf.is_empty() {
                return Ok(got);
            }
            // This member is done; what is left of the input is whatever it did not take.
            let rest = self
                .member
                .take()
                .map(flate2::bufread::GzDecoder::into_inner);
            self.member = rest
                .filter(|r| r.starts_with(&[0x1f, 0x8b]))
                .map(flate2::bufread::GzDecoder::new);
        }
    }
}

/// The magic that begins a snappy stream in the framing the Java client's snappy-java writes: eight bytes, then
/// a version and the oldest compatible version, then blocks each led by its length.
#[cfg(feature = "snappy")]
const XERIAL_MAGIC: [u8; 8] = [0x82, b'S', b'N', b'A', b'P', b'P', b'Y', 0];

/// Snappy as Kafka clients write it: the Java client's xerial framing, or (librdkafka) one plain block.
#[cfg(feature = "snappy")]
fn snappy<'a>(body: &'a [u8], limit: u64) -> Result<Box<dyn Read + 'a>, InflateError> {
    let corrupt = |why: &str| InflateError::Corrupt(format!("snappy: {why}"));
    if let Some(blocks) = body.strip_prefix(&XERIAL_MAGIC) {
        let blocks = blocks
            .get(8..)
            .ok_or_else(|| corrupt("the stream header is cut short"))?;
        return Ok(Box::new(Xerial {
            rest: blocks,
            ready: Vec::new(),
            at: 0,
            left: limit,
        }));
    }
    let len = snap::raw::decompress_len(body).map_err(|e| corrupt(&e.to_string()))?;
    if len as u64 > limit {
        return Err(InflateError::TooLarge);
    }
    let out = snap::raw::Decoder::new()
        .decompress_vec(body)
        .map_err(|e| corrupt(&e.to_string()))?;
    Ok(Box::new(io::Cursor::new(out)))
}

/// The blocks of a xerial snappy stream, inflated one at a time.
#[cfg(feature = "snappy")]
struct Xerial<'a> {
    rest: &'a [u8],
    ready: Vec<u8>,
    at: usize,
    left: u64,
}

#[cfg(feature = "snappy")]
impl Read for Xerial<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        while self.at >= self.ready.len() {
            let Some((len, rest)) = self.rest.split_first_chunk::<4>() else {
                return Ok(0);
            };
            let len = u32::from_be_bytes(*len) as usize;
            let block = rest.get(..len).ok_or_else(|| {
                io::Error::new(io::ErrorKind::UnexpectedEof, "a snappy block is cut short")
            })?;
            self.rest = rest.get(len..).unwrap_or_default();
            let inflated = snap::raw::decompress_len(block)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
            if inflated as u64 > self.left {
                return Err(io::Error::other(Exceeded));
            }
            self.left -= inflated as u64;
            self.ready = snap::raw::Decoder::new()
                .decompress_vec(block)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
            self.at = 0;
        }
        let pending = self.ready.get(self.at..).unwrap_or_default();
        let n = buf.len().min(pending.len());
        if let (Some(dst), Some(src)) = (buf.get_mut(..n), pending.get(..n)) {
            dst.copy_from_slice(src);
        }
        self.at += n;
        Ok(n)
    }
}

/// The window a zstd frame says a decoder needs (RFC 8878, 3.1.1.1), if it is a frame that says.
#[cfg(feature = "zstd")]
fn zstd_declared_window(frame: &[u8]) -> Option<u64> {
    if frame.len() < 6 || frame.get(..4)? != [0x28, 0xb5, 0x2f, 0xfd] {
        return None;
    }
    let descriptor = *frame.get(4)?;
    if descriptor & 0x20 == 0 {
        // Not a single segment: a window descriptor follows.
        let w = u64::from(*frame.get(5)?);
        let base = 1u64 << (10 + (w >> 3));
        return Some(base + base / 8 * (w & 7));
    }
    // A single segment's window is its content size, the next field.
    let size = 1usize << (descriptor >> 6);
    let field = frame.get(5..5 + size)?;
    Some(match size {
        1 => u64::from(*field.first()?),
        2 => u64::from(u16::from_le_bytes(field.try_into().ok()?)) + 256,
        4 => u64::from(u32::from_le_bytes(field.try_into().ok()?)),
        _ => u64::from_le_bytes(field.try_into().ok()?),
    })
}

/// A real compressor declares what its level says and not what the payload needs (librdkafka and the Java client ask
/// for 2 MiB for a 12 KiB report), so no window under that is refused; and the decoder allocates the declared window
/// before it has read a byte, so a frame may not declare more than four times what it may inflate to, nor more than
/// 8 MiB (the largest window of any level up to 19, and what RFC 8878 asks every decoder to take).
#[cfg(feature = "zstd")]
fn zstd<'a>(body: &'a [u8], limit: u64) -> Result<Box<dyn Read + 'a>, InflateError> {
    const MIN_WINDOW: u64 = 2 << 20;
    const MAX_WINDOW: u64 = 8 << 20;
    /// The largest window Apache Kafka's zstd decoder takes.
    const KAFKA_MAX_WINDOW: u64 = 128 << 20;
    let allowed = limit.saturating_mul(4).clamp(MIN_WINDOW, MAX_WINDOW);
    if let Some(window) = zstd_declared_window(body) {
        // Past what Kafka's own decoder takes, Kafka cannot read the frame at all; past what this broker will
        // allocate, it is too large for this broker.
        if window > KAFKA_MAX_WINDOW {
            return Err(InflateError::Corrupt(format!(
                "a zstd frame that declares a window of {window} bytes"
            )));
        }
        if window > allowed {
            return Err(InflateError::TooLarge);
        }
    }
    let decoder = ruzstd::decoding::StreamingDecoder::new(body)
        .map_err(|e| InflateError::Corrupt(format!("zstd: {e}")))?;
    Ok(Box::new(decoder))
}

#[cfg(test)]
mod tests;
