//! Reading a request: from a frame's payload to something a handler can act on.
//!
//! Reading is two steps with a gap between them on purpose. [`header::split`] reads only the header; the
//! server then decides, from the API and the connection's login state, whether to go on; and
//! [`Incoming::decode`] reads the body. A stranger on an unauthenticated connection must never have a
//! body decoded for an API it may not use. [`read`] is both steps for callers with no state to consult.

use bytes::Bytes;
use kafka_protocol::messages::{ApiKey, ApiVersionsRequest};

use crate::api::Request;
use crate::error::WireError;
use crate::header::{self, RequestHead};
use crate::profile::{Features, Profile};

/// A request read from a frame.
#[derive(Debug)]
#[non_exhaustive]
pub enum Incoming {
    /// Any request except ApiVersions, decoded.
    Request {
        /// Its header.
        head: RequestHead,
        /// Its body.
        request: Request,
    },
    /// An ApiVersions request. It is answered from the profile alone ([`crate::api_versions::answer`]), so
    /// the body is kept only if it could be read: a body that cannot be, or a version that is not served,
    /// is answered all the same.
    ApiVersions {
        /// Its header.
        head: RequestHead,
        /// Its body, if it was served and readable.
        body: Option<ApiVersionsRequest>,
    },
}

impl Incoming {
    /// Reads the body of the request whose header is `head`.
    ///
    /// # Errors
    /// Anything a real broker answers by closing the connection: an API or version that was not
    /// advertised, or a body that does not decode. ApiVersions never fails past its header.
    pub fn decode(
        head: RequestHead,
        mut body: Bytes,
        profile: Profile,
        features: Features,
    ) -> Result<Self, WireError> {
        if head.key == ApiKey::ApiVersions {
            let served = profile
                .range(ApiKey::ApiVersions)
                .is_some_and(|(min, max)| (min..=max).contains(&head.version));
            let body = served
                .then(|| Request::decode(head.key, head.version, &mut body).ok())
                .flatten()
                .and_then(|r| match r {
                    Request::ApiVersions(q) => Some(q),
                    _ => None,
                });
            return Ok(Self::ApiVersions { head, body });
        }
        profile.check(head.key, head.version, features)?;
        let request = Request::decode(head.key, head.version, &mut body)?;
        Ok(Self::Request { head, request })
    }
}

// fuzz:parser fuzz_request
/// Reads a request from one frame's payload: the header, then the body, with nothing decided in between.
///
/// # Errors
/// A header that does not decode, or anything [`Incoming::decode`] refuses.
pub fn read(frame: Bytes, profile: Profile, features: Features) -> Result<Incoming, WireError> {
    let (head, body) = header::split(frame)?;
    Incoming::decode(head, body, profile, features)
}

#[cfg(test)]
mod tests;
