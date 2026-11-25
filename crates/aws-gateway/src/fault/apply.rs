//! Asking the rules about a request, and what each path does with the answer.

use std::fmt;

use aws_wire::ApiError;
use http::Response;
use http::request::Parts;

use super::{Done, Failure, Verdict};
use crate::pipeline::Shared;

/// What a path does next.
pub(crate) enum Step {
    /// Serve the request (it may have waited).
    Go,
    /// Answer this error in the protocol's own shape.
    Fail(ApiError),
    /// Close the connection.
    Drop,
}

/// Asks the rules about the request in hand, which has been decoded: its service, operation and resource are on its notes.
pub(crate) async fn apply(shared: &Shared, parts: &Parts) -> Step {
    let Some(faults) = shared.config.faults.as_ref().filter(|f| f.armed()) else {
        return Step::Go;
    };
    match crate::observe::judge(parts, &**faults) {
        Verdict::Pass => Step::Go,
        Verdict::Slow { delay, tag } => {
            crate::observe::lab(parts, &tag, Done::Delayed(delay));
            tokio::time::sleep(delay).await;
            Step::Go
        }
        Verdict::Fail(failure) => {
            crate::observe::lab(parts, &failure.tag, Done::Failed);
            Step::Fail(error_of(*failure))
        }
        Verdict::Drop { tag } => {
            crate::observe::lab(parts, &tag, Done::Dropped);
            Step::Drop
        }
    }
}

/// The error a protocol renders. A shape the model carries is rendered as the model says; one it does not is rendered as stated.
fn error_of(f: Failure) -> ApiError {
    let mut e = ApiError::new(f.status, f.code, f.message);
    e.shape = f.shape.map(std::borrow::Cow::Borrowed);
    e
}

/// What a dropped request is answered with: nothing. The response carries this mark; the connection loop turns it into an error from
/// the service function, which ends the connection without an answer, and the observer is told the call had no status.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Dropped;

impl fmt::Display for Dropped {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a Lab rule closed this connection")
    }
}

impl std::error::Error for Dropped {}

/// A response that stands for a dropped connection.
pub(crate) fn marked_dropped<B: Default>() -> Response<B> {
    let mut response = Response::new(B::default());
    response.extensions_mut().insert(Dropped);
    response
}

/// Whether `response` stands for a dropped connection.
pub(crate) fn is_dropped<B>(response: &Response<B>) -> bool {
    response.extensions().get::<Dropped>().is_some()
}
