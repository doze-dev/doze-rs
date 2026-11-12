//! What every operation handler of a JSON service does the same way: read the request as a typed view, and write an answer.

use aws_schema::arena::{Arena, NIL};
use aws_schema::build::Object;
use aws_schema::view::{Shape, View};

use crate::ApiError;
use crate::service::{Call, Output, Reply};

/// The request as a typed view.
///
/// # Errors
/// An internal error if the call was not decoded (the gateway always decodes first).
pub fn request<S: Shape>(call: &Call) -> Result<View<'_, S>, ApiError> {
    View::<S>::root(&call.arena, &call.body, call.root)
        .ok_or_else(|| ApiError::internal("the request was not decoded"))
}

/// An answer, written by `fill`.
///
/// # Errors
/// Never: the type is a [`Reply`] so a handler can return it directly.
pub fn reply<S>(fill: impl FnOnce(&mut Object<'_, S>)) -> Reply {
    let mut arena = Arena::new();
    let mut out = Object::<S>::root(&mut arena);
    fill(&mut out);
    let root = out.finish();
    Ok(Output { arena, root })
}

/// An answer with nothing in it (the operation has no output).
///
/// # Errors
/// Never.
pub fn empty() -> Reply {
    Ok(Output {
        arena: Arena::new(),
        root: NIL,
    })
}
