//! Every message of protocol 2. Field numbers are frozen here: fields are only added (with a new number, `Option` or
//! `#[cbor(default)]`), never renumbered or removed; enums that may grow travel inside `Option`.

pub mod calls;
pub mod console;
pub mod error;
pub mod hello;
pub mod hints;
pub mod instance;
pub mod run;
pub mod value;

use minicbor::{Decode, Encode};

/// The head of every call body, before its arguments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub struct CallHead {
    /// Milliseconds the caller will wait. The callee should give up when they are spent.
    #[n(0)]
    pub deadline_ms: Option<u32>,
    /// For a callback: the id of the call it serves (a module may call `fetch.*` only for a `resolve` in flight).
    #[n(1)]
    pub for_call: Option<u32>,
}

/// A message could not be encoded or decoded.
#[derive(Debug, thiserror::Error)]
pub enum CodecError {
    #[error("encoding: {0}")]
    Encode(String),
    #[error("decoding: {0}")]
    Decode(#[from] minicbor::decode::Error),
}

/// Encodes one message.
pub fn encode<T: Encode<()>>(v: &T) -> Result<Vec<u8>, CodecError> {
    minicbor::to_vec(v).map_err(|e| CodecError::Encode(e.to_string()))
}

/// Decodes one message.
pub fn decode<'b, T: Decode<'b, ()>>(bytes: &'b [u8]) -> Result<T, CodecError> {
    Ok(minicbor::decode(bytes)?)
}

/// A call body: the head, then the arguments, in one buffer (no copy of the arguments).
pub fn encode_call<T: Encode<()>>(head: CallHead, args: &T) -> Result<Vec<u8>, CodecError> {
    let mut out = encode(&head)?;
    minicbor::encode(args, &mut out).map_err(|e| CodecError::Encode(e.to_string()))?;
    Ok(out)
}

/// Splits a call body into its head and the bytes of its arguments.
pub fn decode_call(body: &[u8]) -> Result<(CallHead, &[u8]), CodecError> {
    let mut d = minicbor::Decoder::new(body);
    let head: CallHead = d.decode()?;
    let args = body.get(d.position()..).unwrap_or_default();
    Ok((head, args))
}
