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

/// An answer written as JSON, for a handler that builds its answer as a `serde_json::Value`: the value is read into the operation's
/// output shape (its structure must fit; the size limits and required members of the model are not held against it). `null` is an answer with nothing in it.
///
/// # Errors
/// An internal error when the value is not a valid output of the operation (a fault of the handler's, not of the caller).
pub fn reply_json(
    tables: &aws_schema::tables::Tables,
    patterns: &aws_schema::patterns::Patterns,
    op: u32,
    value: &serde_json::Value,
) -> Reply {
    let Some(shape) = tables
        .op(op)
        .and_then(|o| o.output)
        .filter(|_| !value.is_null())
    else {
        return empty();
    };
    let mut arena = Arena::new();
    let text = value.to_string();
    let root = aws_schema::json::decode_lenient(tables, patterns, shape, &text, &mut arena)
        .map_err(|v| {
            ApiError::internal(format!(
                "a handler wrote an answer its own model refuses: {v:?}"
            ))
        })?;
    arena.detach(&text);
    Ok(Output { arena, root })
}
