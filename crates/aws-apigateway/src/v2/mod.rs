//! The HTTP and WebSocket API (v2) control plane.

use aws_model_apigatewayv2::Operation;

use crate::core::Core;
use crate::errors::refused;
use crate::req::{Req, Res};

/// The operations built.
pub(crate) fn built(_op: Operation) -> bool {
    false
}

/// Runs operation `op`.
pub(crate) fn dispatch(_core: &mut Core, op: u32, _r: &Req) -> Res {
    let name =
        Operation::from_index(op).map_or_else(|| "This operation".to_owned(), |o| format!("{o:?}"));
    Err(refused(&name))
}
