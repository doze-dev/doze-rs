//! The API of a resource's own pages (a queue's messages, a bucket's objects, a table's items, a function's test, a log
//! group's tail), each call made through the peer bus as the caller `console`, when the page asks:
//!
//! - `POST /api/v1/resources/<service>/<kind>/<name>/op/<Operation>`: one of the operations the kind's pages make
//!   ([`crate::resources::Deep`]), with the page's input (a JSON object); the service's answer as it gave it (Lambda's
//!   `Invoke`: [`crate::model::Invoked`], the payload and log tail as text).
//! - `POST /api/v1/resources/s3/buckets/<name>/object?key=<key>[&props=<json>]`: the request's body put as the object `key`
//!   (PutObject), at most [`UPLOAD_MAX`]; its `Content-Type` is the object's; `props` sets its metadata, tags, encryption and
//!   the like ([`super::objects::put_props`]).
//! - `GET /api/v1/resources/s3/buckets/<name>/object?key=<key>[&versionId=<id>]`: the object (a version of it), streamed as GetObject
//!   answers it.
//! - `GET /api/v1/resources/logs/groups/<name>/tail[?stream=<name>…][&pattern=<text>]`: a Live Tail of the group as
//!   server-sent events ([`deep::tail`]), its request bound to the group as any operation's is; one of the console's event
//!   streams ([`crate::limits::Limits::max_streams`]).
//!
//! The upload and the download are S3's alone: no other kind has bytes to carry.

use std::sync::Arc;

use axum::body::{Body, Bytes};
use axum::extract::{Path, RawQuery, State};
use axum::http::header::{CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::sse::{KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use serde_json::{Map, Value, json};

use super::calls::KEEP_ALIVE;
use super::resources::{decode, find, refused};
use super::{busy, error, json, param};
use crate::model::Uploaded;
use crate::resources::{deep, path};
use crate::state::Shared;

/// The most an upload through the console may be: 25 MiB. A larger object is put with the AWS CLI or an SDK.
pub const UPLOAD_MAX: usize = 25 << 20;

/// `POST /api/v1/resources/<service>/<kind>/<name>/op/<operation>`.
pub async fn op(
    State(s): State<Shared>,
    Path((service, kind, name, operation)): Path<(String, String, String, String)>,
    body: Bytes,
) -> Response {
    let (bus, d, k) = match find(&s, &service, &kind) {
        Ok(found) => found,
        Err(res) => return *res,
    };
    let input = if body.is_empty() {
        Map::new()
    } else {
        match serde_json::from_slice::<Value>(&body) {
            Ok(Value::Object(input)) => input,
            _ => {
                return error(
                    StatusCode::BAD_REQUEST,
                    "BAD_REQUEST",
                    "An operation's input is a JSON object.",
                );
            }
        }
    };
    match deep::call(&bus, (d, k), &name, &operation, input).await {
        Ok(answer) => json(StatusCode::OK, &answer),
        Err(e) => refused(&e),
    }
}

/// The bucket and key an object address names, or the answer that says why it names none.
fn object_of(service: &str, kind: &str, query: Option<&str>) -> Result<String, Box<Response>> {
    if (service, kind) != ("s3", "buckets") {
        return Err(Box::new(error(
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            "Only a bucket has objects to upload or download.",
        )));
    }
    match param(query, "key").map(decode) {
        Some(key) if !key.is_empty() => Ok(key),
        _ => Err(Box::new(error(
            StatusCode::BAD_REQUEST,
            "BAD_REQUEST",
            "Name the object with ?key=.",
        ))),
    }
}

/// `POST /api/v1/resources/s3/buckets/<name>/object?key=`.
pub async fn upload(
    State(s): State<Shared>,
    Path((service, kind, name)): Path<(String, String, String)>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let key = match object_of(&service, &kind, query.as_deref()) {
        Ok(key) => key,
        Err(res) => return *res,
    };
    let (bus, d, _) = match find(&s, &service, &kind) {
        Ok(found) => found,
        Err(res) => return *res,
    };
    let props = match super::objects::put_props(query.as_deref(), decode) {
        Ok(p) => p,
        Err(why) => return error(StatusCode::BAD_REQUEST, "BAD_REQUEST", &why),
    };
    let size = body.len() as u64;
    let mut input = props;
    input.insert("Bucket".to_owned(), Value::String(name));
    input.insert("Key".to_owned(), Value::String(key.clone()));
    input.insert("ContentLength".to_owned(), Value::from(size));
    if let Some(t) = headers.get(CONTENT_TYPE).and_then(|v| v.to_str().ok())
        && !t.is_empty()
    {
        input.insert("ContentType".to_owned(), Value::String(t.to_owned()));
    }
    let input = Value::Object(input);
    match bus
        .call_rest(d.target, "PutObject", &input, Some(body))
        .await
    {
        Ok(reply) => json(
            StatusCode::CREATED,
            &Uploaded {
                key,
                size,
                etag: path::text(&reply.json, "ETag"),
                version_id: path::text(&reply.json, "VersionId"),
                operation: "PutObject".to_owned(),
            },
        ),
        Err(e) => refused(&e),
    }
}

/// `GET /api/v1/resources/s3/buckets/<name>/object?key=`: the bytes as S3 streams them, never held whole here.
pub async fn download(
    State(s): State<Shared>,
    Path((service, kind, name)): Path<(String, String, String)>,
    RawQuery(query): RawQuery,
) -> Response {
    let key = match object_of(&service, &kind, query.as_deref()) {
        Ok(key) => key,
        Err(res) => return *res,
    };
    let (bus, d, _) = match find(&s, &service, &kind) {
        Ok(found) => found,
        Err(res) => return *res,
    };
    let mut input = json!({ "Bucket": name, "Key": key });
    // `&versionId=` downloads one version of a versioned bucket's object.
    if let (Some(v), Some(o)) = (param(query.as_deref(), "versionId").map(decode), input.as_object_mut())
        && !v.is_empty()
    {
        o.insert("VersionId".to_owned(), Value::String(v));
    }
    let reply = match bus.call_rest(d.target, "GetObject", &input, None).await {
        Ok(reply) => reply,
        Err(e) => return refused(&e),
    };
    let body = match reply.body {
        Some(mut stream) => Body::from_stream(futures_util::stream::poll_fn(move |cx| {
            stream.as_mut().poll_next(cx)
        })),
        None => Body::empty(),
    };
    let mut res = (StatusCode::OK, body).into_response();
    let h = res.headers_mut();
    let kind = path::text(&reply.json, "ContentType")
        .unwrap_or_else(|| "application/octet-stream".to_owned());
    if let Ok(v) = HeaderValue::from_str(&kind) {
        h.insert(CONTENT_TYPE, v);
    }
    if let Some(n) = path::text(&reply.json, "ContentLength")
        && let Ok(v) = HeaderValue::from_str(&n)
    {
        h.insert(CONTENT_LENGTH, v);
    }
    if let Ok(v) = HeaderValue::from_str(&disposition(&key)) {
        h.insert(CONTENT_DISPOSITION, v);
    }
    res
}

/// `GET /api/v1/resources/<service>/<kind>/<name>/tail`: the kind's streamed operation (a log group's `StartLiveTail`), bound
/// to the resource `name` by its descriptor as every page's operation is.
pub async fn tail(
    State(s): State<Shared>,
    Path((service, kind, name)): Path<(String, String, String)>,
    RawQuery(query): RawQuery,
) -> Response {
    let (bus, d, k) = match find(&s, &service, &kind) {
        Ok(found) => found,
        Err(res) => return *res,
    };
    let Some(op) = deep::STREAMED.iter().find_map(|o| deep::allowed(k, o)) else {
        return refused(&deep::not_made(k, "a tail"));
    };
    let Ok(slot) = Arc::clone(&s.streams).try_acquire_owned() else {
        return busy(
            "The console has as many event streams open as it allows. Close a tail in another tab and start this one again.",
        );
    };
    let ask = deep::tail::ask_of(query.as_deref(), decode);
    let body = match deep::id_for(&bus, (d, k), op, &name).await {
        Ok(id) => deep::bound(&bus, op, (&id, &name), deep::tail::input(&ask)),
        Err(e) => Err(e),
    };
    let body = match body {
        Ok(body) => Value::Object(body),
        Err(e) => return refused(&e),
    };
    match bus.stream(d.target, op.op, &body).await {
        Ok(download) => Sse::new(deep::tail::events(download, s.stop.clone(), slot))
            .keep_alive(KeepAlive::new().interval(KEEP_ALIVE).text("keep-alive"))
            .into_response(),
        Err(e) => refused(&e),
    }
}

/// `Content-Disposition` for saving the object `key` as a file named by its last part: a plain name for old readers (only
/// letters, digits and `.-_`), and the whole name encoded (RFC 8187).
#[must_use]
pub fn disposition(key: &str) -> String {
    let file = key.rsplit('/').find(|p| !p.is_empty()).unwrap_or("object");
    let plain: String = file
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let mut encoded = String::with_capacity(file.len() * 3);
    for b in file.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_') {
            encoded.push(char::from(b));
        } else {
            let hex =
                |n: u8| char::from_digit(u32::from(n), 16).map_or('0', |c| c.to_ascii_uppercase());
            encoded.extend(['%', hex(b >> 4), hex(b & 15)]);
        }
    }
    format!("attachment; filename=\"{plain}\"; filename*=UTF-8''{encoded}")
}

#[cfg(test)]
mod tests {
    use super::disposition;

    #[test]
    fn a_download_is_saved_under_the_last_part_of_its_key() {
        assert_eq!(
            disposition("banners/hero.jpg"),
            "attachment; filename=\"hero.jpg\"; filename*=UTF-8''hero.jpg"
        );
        assert_eq!(
            disposition("a/\"x\" y.txt"),
            "attachment; filename=\"_x__y.txt\"; filename*=UTF-8''%22x%22%20y.txt"
        );
        assert_eq!(
            disposition("folder/"),
            "attachment; filename=\"folder\"; filename*=UTF-8''folder"
        );
        assert_eq!(
            disposition("caf\u{e9}.txt"),
            "attachment; filename=\"caf_.txt\"; filename*=UTF-8''caf%C3%A9.txt"
        );
    }
}
