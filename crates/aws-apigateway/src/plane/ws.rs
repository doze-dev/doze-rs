//! WebSocket APIs.

use std::sync::Arc;

use aws_wire::service::{WebReply, WebRequest};

use crate::Inner;

/// The open connections.
#[derive(Debug, Default)]
pub(crate) struct Connections {}

/// `<stage>/@connections/<id>` of a management request.
pub(crate) fn management_path(rest: &str) -> Option<(&str, &str)> {
    let (stage, after) = rest.split_once('/')?;
    let id = after.strip_prefix("@connections/")?;
    Some((stage, id))
}

pub(crate) async fn manage(
    _inner: &Arc<Inner>,
    _api: &str,
    _stage: &str,
    _conn: &str,
    _request: WebRequest,
) -> WebReply {
    super::reply::message(501, "The @connections API is not built yet")
}
