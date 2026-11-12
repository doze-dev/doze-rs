//! The handler side of overlays (`ui::overlay`, `docs/port/console/overlays.md`): an overlay's route answers the overlay
//! element alone to a script (`X-Fragment`), and the PARENT page with the overlay open on it to a browser, so the address
//! works on a direct visit, on a reload, and with no script at all.

use axum::http::header::CONTENT_TYPE;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use maud::Markup;

use crate::state::Shared;
use crate::ui::client::{HEADER_FRAGMENT, HEADER_OVERLAY};
use crate::ui::overlay::over;
use crate::ui::shell::Rail;

/// The page behind an overlay: what its handler would have rendered for the parent's own address.
#[derive(Debug)]
pub(super) struct Parent {
    pub rail: Rail,
    /// The document title: the parent's own.
    pub title: String,
    /// The breadcrumb: label and path under the base; the last has no path.
    pub crumbs: Vec<(String, Option<String>)>,
    /// The fragments the parent asks the stream to watch, so that it stays live behind the overlay.
    pub watch: &'static [&'static str],
    pub main: Markup,
}

/// Whether the request is a script's: it wants the overlay element alone (an htmx request that targets the overlay root, or a
/// request that says so with `X-Fragment`).
#[must_use]
pub(super) fn wants_fragment(headers: &HeaderMap) -> bool {
    headers.contains_key(HEADER_FRAGMENT)
        || headers
            .get(crate::region::HX_TARGET)
            .is_some_and(|t| t.as_bytes() == crate::region::OVERLAY_ROOT.as_bytes())
}

/// The overlay element alone, marked so that a script knows it is an overlay and not a page. A script reads the element and the
/// mark, never the status, and a browser logs every 4xx a fetch gets as an error in its console: so a field to fix (422) is a
/// 200 to a script, while the same answer to a plain POST keeps its 422.
#[must_use]
pub(super) fn fragment(status: StatusCode, overlay: &Markup) -> Response {
    let status = if status == StatusCode::UNPROCESSABLE_ENTITY {
        StatusCode::OK
    } else {
        status
    };
    (
        status,
        [
            (
                CONTENT_TYPE,
                HeaderValue::from_static("text/html; charset=utf-8"),
            ),
            (
                axum::http::HeaderName::from_static(HEADER_OVERLAY),
                HeaderValue::from_static("1"),
            ),
        ],
        overlay.0.clone(),
    )
        .into_response()
}

/// An overlay over its parent, as a whole document. (A script is answered by [`fragment`] before the parent is built, so
/// that a script asking for a record does not make the server read the whole Messages page too.)
pub(super) async fn over_parent(
    s: &Shared,
    status: StatusCode,
    p: Parent,
    overlay: Markup,
) -> Response {
    let doc = s
        .document_over(
            p.rail,
            &p.title,
            &crate::trail::borrowed(&p.crumbs),
            p.watch,
            over(p.main, overlay),
        )
        .await;
    (status, super::html(&doc)).into_response()
}
