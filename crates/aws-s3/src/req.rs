//! A request as a handler reads it: the input as JSON, who is calling, and the body already summed when there was one.

use std::sync::Arc;

use aws_wire::ApiError;
use aws_wire::rest::Download;
use aws_wire::service::Call;
use doze_fs::fs::{Fs, RealFs};
use serde_json::Value;

use crate::blob::Staging;
use crate::checksum::Done;

/// A body that has been written to disk and summed, waiting for its metadata.
pub(crate) struct Staged {
    pub(crate) staging: Staging<RealFs>,
    pub(crate) done: Done,
}

/// What the actor is asked.
pub(crate) enum Ask {
    /// Is the caller allowed, and is the target there? Asked before a body is read.
    Gate(Call),
    /// Do the operation.
    Call(Call, Option<Staged>),
    /// Apply the lifecycle rules of every bucket as of now (the janitor asks; so does a test).
    Sweep,
}

/// A stretch of a blob to send.
pub(crate) struct Slice {
    pub(crate) file: Arc<<RealFs as Fs>::File>,
    pub(crate) from: u64,
    pub(crate) len: u64,
}

/// What an operation produced.
#[derive(Default)]
pub(crate) struct Out {
    pub(crate) json: Value,
    pub(crate) status: Option<u16>,
    pub(crate) body: Option<Vec<Slice>>,
}

pub(crate) type Answer = Result<Out, ApiError>;

impl Out {
    pub(crate) fn json(v: Value) -> Answer {
        Ok(Self {
            json: v,
            ..Self::default()
        })
    }

    pub(crate) fn empty() -> Answer {
        Self::json(Value::Object(serde_json::Map::new()))
    }

    /// An answer with no output and status 204 (some Put operations answer so although the model says 200).
    pub(crate) fn no_content() -> Answer {
        Ok(Self {
            json: Value::Object(serde_json::Map::new()),
            status: Some(204),
            body: None,
        })
    }
}

/// The streamed body type, for the signature of the wrapper.
pub(crate) type Stream = Download;

/// A request being served.
pub(crate) struct Req<'a> {
    pub(crate) call: &'a Call,
    pub(crate) v: Value,
    pub(crate) staged: Option<Staged>,
}

impl<'a> Req<'a> {
    pub(crate) fn new(call: &'a Call, staged: Option<Staged>) -> Self {
        let v = serde_json::from_str(&call.body).unwrap_or(Value::Null);
        Self { call, v, staged }
    }

    pub(crate) fn s(&self, name: &str) -> Option<&str> {
        self.v.get(name).and_then(Value::as_str)
    }

    pub(crate) fn int(&self, name: &str) -> Option<i64> {
        self.v.get(name).and_then(Value::as_i64)
    }

    pub(crate) fn flag(&self, name: &str) -> Option<bool> {
        self.v.get(name).and_then(Value::as_bool)
    }

    pub(crate) fn get(&self, name: &str) -> Option<&Value> {
        self.v.get(name)
    }

    /// The bucket the request names (every bucket operation has one).
    pub(crate) fn bucket(&self) -> &str {
        self.s("Bucket").unwrap_or_default()
    }

    pub(crate) fn key(&self) -> &str {
        self.s("Key").unwrap_or_default()
    }

    pub(crate) fn region(&self) -> &str {
        &self.call.caller.region
    }

    pub(crate) fn account(&self) -> &str {
        &self.call.caller.account_id
    }
}
