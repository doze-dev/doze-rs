//! The execute-api plane.

use std::sync::Arc;

use aws_wire::service::{WebReply, WebRequest};

use crate::Inner;
use crate::core::{Api, Core};

/// What the plane keeps beside the actors.
#[derive(Debug, Default)]
pub(crate) struct State {}

impl State {
    pub(crate) fn after_change(&self, _api: Api, _op: u32) {}
}

#[derive(Debug)]
pub(crate) struct Find {}

#[derive(Debug)]
pub(crate) struct Found {}

#[derive(Debug)]
pub(crate) enum Miss {
    NoApi,
}

impl Core {
    pub(crate) fn find(&self, _f: &Find) -> Result<Found, Miss> {
        Err(Miss::NoApi)
    }
}

pub(crate) fn owns(_host: &str, path: &str) -> bool {
    path.starts_with("/_aws/execute-api/")
}

pub(crate) async fn serve(_inner: Arc<Inner>, _request: WebRequest) -> WebReply {
    WebReply {
        status: 404,
        headers: Vec::new(),
        body: bytes::Bytes::new(),
    }
}
