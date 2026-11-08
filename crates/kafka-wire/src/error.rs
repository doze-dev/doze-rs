//! What can go wrong between the socket and a handler.

use kafka_protocol::messages::ApiKey;

/// A request that cannot be served. Each of these closes the connection (a real broker does the same),
/// except where a caller is documented to answer it instead.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum WireError {
    /// A frame of length zero.
    #[error("zero-length frame")]
    EmptyFrame,
    /// A frame too short to hold the two bytes of API key that choose its size cap.
    #[error("a frame of {0} bytes cannot hold a request")]
    FrameTooShort(u32),
    /// A frame over the cap for its API (or over every cap).
    #[error("frame of {len} bytes exceeds the {limit} allowed")]
    FrameTooLarge {
        /// The length the frame claimed.
        len: u32,
        /// The most that was allowed for it.
        limit: u32,
    },
    /// An API key this crate has no name for.
    #[error("unknown API key {0}")]
    UnknownApi(i16),
    /// An API this broker never serves.
    #[error("API {0:?} is not served")]
    NotServed(ApiKey),
    /// A version outside what the profile advertises for the API.
    #[error("{key:?} v{version} is outside the advertised versions")]
    NotAdvertised {
        /// The API.
        key: ApiKey,
        /// The version asked for.
        version: i16,
    },
    /// A header or body that does not decode.
    #[error("cannot decode {what}: {reason}")]
    Decode {
        /// What was being decoded.
        what: &'static str,
        /// Why not.
        reason: String,
    },
    /// A response that cannot be encoded.
    #[error("cannot encode {what}: {reason}")]
    Encode {
        /// What was being encoded.
        what: &'static str,
        /// Why not.
        reason: String,
    },
}

impl WireError {
    pub(crate) fn decode(what: &'static str, e: impl std::fmt::Display) -> Self {
        Self::Decode {
            what,
            reason: e.to_string(),
        }
    }

    pub(crate) fn encode(what: &'static str, e: impl std::fmt::Display) -> Self {
        Self::Encode {
            what,
            reason: e.to_string(),
        }
    }
}
