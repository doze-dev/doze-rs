//! The Resources space's API: each answer made by a descriptor's calls through the peer bus, as the caller `console`, when it
//! is asked (nothing is polled or kept).
//!
//! - `GET /api/v1/resources[?counts=no]`: every service, its kinds, how many of each (and their names, for the palette).
//! - `GET /api/v1/resources/<service>/<kind>[?token=][&all=yes]`: a page of a kind (with `all`, the rows it sets aside too).
//! - `POST /api/v1/resources/<service>/<kind>`: create one from the form's values (a JSON object).
//! - `GET /api/v1/resources/<service>/<kind>/<name>`: one, with its lists and its recent calls.
//! - `DELETE /api/v1/resources/<service>/<kind>/<name>[?clear=yes]`: delete it (with `clear`, what the kind can clear of what
//!   stands in the way goes first).
//! - `GET …/<name>/tags`, `PUT …/<name>/tags` ([`super::tags`]), `GET …/<name>/blockers` ([`super::blockers`]) and `POST
//!   …/<kind>/op/<Operation>` ([`super::ops`]), each in a module of its own.
//! - `POST /api/v1/resources/<service>/<kind>/<name>/reveal`: its hidden value (a POST: never prefetched, and behind the
//!   same-origin proof as a write).
//!
//! A refusal by the service is answered with its own status, code and words.

use axum::body::Bytes;
use axum::extract::{Path, RawQuery, State};
use axum::http::StatusCode;
use axum::response::Response;
use serde_json::{Map, Value};

use super::{error, json, param};
use crate::resources::bus::Bus;
use crate::resources::{Descriptor, Kind, act, blockers, catalog, params, view};
use crate::state::Shared;

/// The bus and the descriptor and kind an address names, or the answer that says why there are none.
pub(super) fn find(
    s: &Shared,
    service: &str,
    kind: &str,
) -> Result<(Bus, &'static Descriptor, &'static Kind), Box<Response>> {
    let d = catalog::descriptor(service).ok_or_else(|| {
        no(
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            &format!("The console does not describe the resources of {service} yet."),
        )
    })?;
    let k = d.kinds.iter().find(|k| k.id == kind).ok_or_else(|| {
        no(
            StatusCode::NOT_FOUND,
            "NO_SUCH_KIND",
            &format!("{service} has no kind of resource called {kind}."),
        )
    })?;
    if !view::served(&s.facts, service) {
        return Err(no(
            StatusCode::CONFLICT,
            "NOT_SERVED",
            &format!(
                "This doze-aws does not serve {service} (--services), so the console leaves it asleep."
            ),
        ));
    }
    Ok((bus(s)?, d, k))
}

fn bus(s: &Shared) -> Result<Bus, Box<Response>> {
    s.bus().ok_or_else(|| {
        no(
            StatusCode::SERVICE_UNAVAILABLE,
            "NO_BUS",
            "This console was started without the services' bus, so it cannot read resources.",
        )
    })
}

/// A refusal of the console's own, boxed (a response is large to carry in an `Err`).
fn no(status: StatusCode, code: &str, message: &str) -> Box<Response> {
    Box::new(error(status, code, message))
}

/// A service's refusal, as an answer.
pub(super) fn refused(e: &aws_wire::ApiError) -> Response {
    let status = StatusCode::from_u16(e.status).unwrap_or(StatusCode::BAD_GATEWAY);
    error(status, &e.code, &e.message)
}

/// `GET /api/v1/resources[?counts=no]`: with `counts=no`, the services and kinds alone, with no call made (what the pages need
/// to know which kinds there are).
pub async fn index(State(s): State<Shared>, RawQuery(query): RawQuery) -> Response {
    let counted = param(query.as_deref(), "counts") != Some("no");
    match bus(&s) {
        Ok(bus) => json(StatusCode::OK, &view::index(&bus, &s.facts, counted).await),
        Err(res) => *res,
    }
}

/// `GET /api/v1/resources/<service>/<kind>`.
pub async fn list(
    State(s): State<Shared>,
    Path((service, kind)): Path<(String, String)>,
    RawQuery(query): RawQuery,
) -> Response {
    let (bus, d, k) = match find(&s, &service, &kind) {
        Ok(found) => found,
        Err(res) => return *res,
    };
    let token = param(query.as_deref(), "token").map(decode);
    let all = param(query.as_deref(), "all") == Some("yes");
    let given = params::given(k, query.as_deref());
    match view::list(&bus, &s.facts, (d, k), (token.as_deref(), all, &given)).await {
        Ok(page) => json(StatusCode::OK, &page),
        Err(e) => refused(&e),
    }
}

/// `GET /api/v1/resources/<service>/<kind>/<name>`.
pub async fn detail(
    State(s): State<Shared>,
    Path((service, kind, name)): Path<(String, String, String)>,
) -> Response {
    let (bus, d, k) = match find(&s, &service, &kind) {
        Ok(found) => found,
        Err(res) => return *res,
    };
    match view::detail(&bus, (&s.facts, &s.recorder), (d, k), &name).await {
        Ok(one) => json(StatusCode::OK, &one),
        Err(e) => refused(&e),
    }
}

/// `POST /api/v1/resources/<service>/<kind>`.
pub async fn create(
    State(s): State<Shared>,
    Path((service, kind)): Path<(String, String)>,
    body: Bytes,
) -> Response {
    let (bus, d, k) = match find(&s, &service, &kind) {
        Ok(found) => found,
        Err(res) => return *res,
    };
    let Ok(Value::Object(form)) = serde_json::from_slice::<Value>(&body) else {
        return error(
            StatusCode::BAD_REQUEST,
            "BAD_REQUEST",
            "A create is a JSON object of the form's values.",
        );
    };
    let form: Map<String, Value> = match act::checked(k, &form) {
        Ok(f) => f,
        Err(e) => return refused(&e),
    };
    match act::create(&bus, (d, k), &form).await {
        Ok(made) => json(StatusCode::CREATED, &made),
        Err(e) => refused(&e),
    }
}

/// `DELETE /api/v1/resources/<service>/<kind>/<name>`.
pub async fn delete(
    State(s): State<Shared>,
    Path((service, kind, name)): Path<(String, String, String)>,
    RawQuery(query): RawQuery,
) -> Response {
    let (bus, d, k) = match find(&s, &service, &kind) {
        Ok(found) => found,
        Err(res) => return *res,
    };
    // `?clear=yes`: what the kind can clear of what stands in the way goes first.
    let cleared = if param(query.as_deref(), "clear") == Some("yes") {
        match blockers::clear(&bus, (d, k), &name).await {
            Ok(done) => done,
            Err(e) => return refused(&e),
        }
    } else {
        Vec::new()
    };
    match act::delete(&bus, (d, k), &name).await {
        Ok(done) => json(StatusCode::OK, &crate::model::Deleted { cleared, ..done }),
        Err(e) => refused(&e),
    }
}

/// `POST /api/v1/resources/<service>/<kind>/<name>/reveal`.
pub async fn reveal(
    State(s): State<Shared>,
    Path((service, kind, name)): Path<(String, String, String)>,
) -> Response {
    let (bus, d, k) = match find(&s, &service, &kind) {
        Ok(found) => found,
        Err(res) => return *res,
    };
    match act::reveal(&bus, (d, k), &name).await {
        Ok(value) => json(StatusCode::OK, &value),
        Err(e) => refused(&e),
    }
}

/// `text` with its `%XX` escapes decoded (a page token is written into the address with `encodeURIComponent`); a `+` stays a
/// `+` (that is what `encodeURIComponent` leaves of a space: `%20`). An escape that is not one is kept as written.
#[must_use]
pub fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        let hex = |at: usize| bytes.get(at).and_then(|c| (*c as char).to_digit(16));
        match (b, hex(i + 1), hex(i + 2)) {
            (b'%', Some(hi), Some(lo)) => {
                out.push(u8::try_from(hi * 16 + lo).unwrap_or(b'?'));
                i += 3;
            }
            _ => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::decode;

    #[test]
    fn a_token_is_read_back_as_it_was_written() {
        assert_eq!(decode("abc%2Bdef%3D%3D"), "abc+def==");
        assert_eq!(decode("a%2Fb%20c"), "a/b c");
        assert_eq!(decode("100%"), "100%");
        assert_eq!(decode("%zz%4"), "%zz%4");
        assert_eq!(decode("%E2%9C%93"), "\u{2713}");
    }
}
