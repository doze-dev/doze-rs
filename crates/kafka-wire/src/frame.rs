//! Length-prefixed frames: a 4-byte big-endian length, then that many bytes.
//!
//! The decoder is sans-IO. A caller keeps a `BytesMut` it reads socket bytes into and calls
//! [`FrameDecoder::decode`] after each read; a complete frame comes out, or `None` while more is needed.

use bytes::{Buf, Bytes, BytesMut};
use kafka_protocol::messages::ApiKey;

use crate::error::WireError;

/// What a request may weigh unless it carries record data. Decoding turns each small wire element into a
/// larger struct, so the smaller the frame a stranger may send, the less a hostile one costs.
const CONTROL_CAP: u32 = 1 << 20;
/// Produce is the only request that carries record data; Kafka's own default maximum request size.
const PRODUCE_CAP: u32 = 100 << 20;
/// What a stranger may send before it has logged in. Kafka's `sasl.server.max.receive.size` is 512 KiB; this broker is
/// stricter on purpose (decision D10, SEC-4): 1000 strangers each holding most of 512 KiB were 320 MB, and an ApiVersions
/// request, a SaslHandshake and a login token (under 1 KiB) are all far under 64 KiB.
const ANONYMOUS_CAP: u32 = 64 << 10;
/// PushTelemetry: the 1 MiB of metrics a client is told it may send, and room for the header around it.
const PUSH_TELEMETRY_CAP: u32 = (1 << 20) + 4096;
/// How much of a frame is trusted up front. A frame bigger than this is buffered as its bytes arrive, so
/// memory follows what the peer sends and not what its length prefix claims.
const CHUNK: usize = 64 << 10;

/// How far the peer of a connection is trusted. An anonymous one may send only what ApiVersions and the
/// SASL exchange need, which is far under 64 KiB, whatever the API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trust {
    /// Not yet logged in.
    Anonymous,
    /// Logged in, or on a broker that needs no login.
    Authenticated,
}

/// The most a request to `key` may weigh from a peer trusted as `trust`.
fn cap_for(key: i16, trust: Trust) -> u32 {
    let cap = if key == ApiKey::Produce as i16 {
        PRODUCE_CAP
    } else if key == ApiKey::PushTelemetry as i16 {
        PUSH_TELEMETRY_CAP
    } else {
        CONTROL_CAP
    };
    match trust {
        Trust::Authenticated => cap,
        Trust::Anonymous => cap.min(ANONYMOUS_CAP),
    }
}

/// Splits a byte stream into request frames.
///
/// After an `Err` the stream is not usable: the decoder keeps answering the same error, and the caller
/// closes the connection.
#[derive(Debug)]
pub struct FrameDecoder {
    trust: Trust,
}

impl FrameDecoder {
    /// A decoder for a connection trusted as `trust`.
    #[must_use]
    pub fn new(trust: Trust) -> Self {
        Self { trust }
    }

    /// The connection has logged in: it may now send frames up to each API's own cap.
    pub fn set_authenticated(&mut self) {
        self.trust = Trust::Authenticated;
    }

    /// Whether the peer is trusted (logged in, or on a broker that needs no login).
    #[must_use]
    pub fn is_authenticated(&self) -> bool {
        self.trust == Trust::Authenticated
    }

    /// The length of the frame at the front of `src`, once enough of it has arrived to judge: how much a frame
    /// will weigh is known, and held to its cap, before any of its body is waited for.
    ///
    /// # Errors
    /// A frame that is empty, too short for a key, or over its cap.
    pub fn length(&self, src: &[u8]) -> Result<Option<u32>, WireError> {
        let Some(len) = src.first_chunk::<4>().map(|b| u32::from_be_bytes(*b)) else {
            return Ok(None);
        };
        if len == 0 {
            return Err(WireError::EmptyFrame);
        }
        let most = cap_for(ApiKey::Produce as i16, self.trust);
        if len > most {
            return Err(WireError::FrameTooLarge { len, limit: most });
        }
        if len < 2 {
            return Err(WireError::FrameTooShort(len));
        }
        let Some(key) = src
            .get(4..)
            .and_then(|b| b.first_chunk::<2>())
            .map(|b| i16::from_be_bytes(*b))
        else {
            return Ok(None);
        };
        let limit = cap_for(key, self.trust);
        if len > limit {
            return Err(WireError::FrameTooLarge { len, limit });
        }
        Ok(Some(len))
    }

    // fuzz:parser fuzz_frame
    /// Takes one complete frame off the front of `src`, returning its payload without the length prefix.
    ///
    /// # Errors
    /// A frame that is empty, too short for a key, or over its cap. The length is judged before any of
    /// the body is waited for.
    pub fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Bytes>, WireError> {
        let Some(len) = self.length(src)? else {
            return Ok(None);
        };
        let total = 4 + len as usize;
        if src.len() < total {
            src.reserve((total - src.len()).min(CHUNK));
            return Ok(None);
        }
        src.advance(4);
        Ok(Some(src.split_to(len as usize).freeze()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(len: u32, key: i16) -> BytesMut {
        let mut b = BytesMut::new();
        b.extend_from_slice(&len.to_be_bytes());
        b.extend_from_slice(&key.to_be_bytes());
        b
    }

    #[test]
    fn a_zero_length_frame_is_refused() {
        let mut src = BytesMut::from(&[0u8, 0, 0, 0][..]);
        assert_eq!(
            FrameDecoder::new(Trust::Authenticated).decode(&mut src),
            Err(WireError::EmptyFrame)
        );
    }

    #[test]
    fn an_unauthenticated_connection_is_capped_at_64_kib() {
        // Stricter than Kafka's `sasl.server.max.receive.size` (512 KiB), and than the 1 MiB it was.
        let mut src = header(2 << 20, ApiKey::Produce as i16);
        let mut d = FrameDecoder::new(Trust::Anonymous);
        assert_eq!(
            d.decode(&mut src),
            Err(WireError::FrameTooLarge {
                len: 2 << 20,
                limit: ANONYMOUS_CAP
            })
        );
        assert_eq!(ANONYMOUS_CAP, 64 << 10);
        assert!(!d.is_authenticated());
        d.set_authenticated();
        let mut src = header(2 << 20, ApiKey::Produce as i16);
        assert_eq!(d.decode(&mut src), Ok(None));
    }

    #[test]
    fn a_length_over_every_cap_needs_no_key() {
        // Only the four length bytes have arrived.
        let mut src = BytesMut::from(&(PRODUCE_CAP + 1).to_be_bytes()[..]);
        assert!(matches!(
            FrameDecoder::new(Trust::Authenticated).decode(&mut src),
            Err(WireError::FrameTooLarge { .. })
        ));
    }

    /// The literal sizes, not the constants: a cap that drifts is a change to what clients may send.
    #[test]
    fn the_caps_are_exact() {
        const MIB: u32 = 1 << 20;
        let cases = [
            (ApiKey::Metadata, MIB),
            (ApiKey::ApiVersions, MIB),
            (ApiKey::PushTelemetry, MIB + 4096),
            (ApiKey::Produce, 100 * MIB),
        ];
        for (key, cap) in cases {
            let mut d = FrameDecoder::new(Trust::Authenticated);
            assert_eq!(
                d.decode(&mut header(cap, key as i16)),
                Ok(None),
                "{key:?} at {cap}"
            );
            assert_eq!(
                d.decode(&mut header(cap + 1, key as i16)),
                Err(WireError::FrameTooLarge {
                    len: cap + 1,
                    limit: cap
                }),
                "{key:?} over {cap}"
            );
        }
        // Anonymous, every API is held to 64 KiB.
        const SIXTY_FOUR_KIB: u32 = MIB / 16;
        let mut d = FrameDecoder::new(Trust::Anonymous);
        for key in [
            ApiKey::Produce,
            ApiKey::ApiVersions,
            ApiKey::SaslAuthenticate,
        ] {
            assert_eq!(
                d.decode(&mut header(SIXTY_FOUR_KIB, key as i16)),
                Ok(None),
                "{key:?}"
            );
            assert_eq!(
                d.decode(&mut header(SIXTY_FOUR_KIB + 1, key as i16)),
                Err(WireError::FrameTooLarge {
                    len: SIXTY_FOUR_KIB + 1,
                    limit: SIXTY_FOUR_KIB
                }),
                "{key:?}"
            );
        }
    }

    #[test]
    fn a_length_too_short_for_a_key_is_refused() {
        let mut src = BytesMut::from(&[0u8, 0, 0, 1, 7][..]);
        assert_eq!(
            FrameDecoder::new(Trust::Authenticated).decode(&mut src),
            Err(WireError::FrameTooShort(1))
        );
    }

    #[test]
    fn memory_follows_the_bytes_that_arrive() {
        let mut src = header(PRODUCE_CAP, ApiKey::Produce as i16);
        assert_eq!(
            FrameDecoder::new(Trust::Authenticated).decode(&mut src),
            Ok(None)
        );
        assert!(
            src.capacity() <= 2 * CHUNK,
            "reserved {} bytes for a frame of nothing",
            src.capacity()
        );
    }
}
