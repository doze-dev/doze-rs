//! Try: a JSON value encoded with a version of a subject, nothing sent; and the encoding the Produce dialog uses.

use axum::body::Bytes;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;
use kafka_registry::{Registry, RegistryError};
use serde::{Deserialize, Serialize};

use super::off;
use super::write::bad_body;
use crate::api::json;
use crate::state::Shared;

/// `POST /api/v1/schemas/try`: a JSON value encoded with a version.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TryPost {
    pub subject: String,
    /// A number, or `latest`.
    pub version: String,
    pub value: String,
}

/// A value encoded (or why not): what a serializer would send.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Encoded {
    pub ok: bool,
    /// Why the value does not fit, in words.
    pub error: Option<String>,
    pub id: u32,
    pub version: u32,
    pub kind: String,
    pub subject: String,
    /// The bytes, header included.
    pub size: usize,
    /// The bytes as hex pairs, at most 256 of them shown.
    pub hex: String,
    /// The value as the codec reads it back, laid out.
    pub json: Option<String>,
}

/// Encodes `value` with a version of `subject`.
pub(crate) fn encode(r: &Registry, subject: &str, version: &str, value: &str) -> Result<(Encoded, Vec<u8>), RegistryError> {
    let (c, number) = r.codec_for(subject, version)?;
    let mut out = Encoded {
        ok: false,
        error: None,
        id: c.id(),
        version: number,
        kind: c.kind().name().to_owned(),
        subject: subject.to_owned(),
        size: 0,
        hex: String::new(),
        json: None,
    };
    let bytes = match c.encode(value) {
        Ok(b) => b,
        Err(why) => {
            out.error = Some(why);
            return Ok((out, Vec::new()));
        }
    };
    out.ok = true;
    out.size = bytes.len();
    out.hex = bytes.iter().take(256).map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ");
    out.json = c.decode(&bytes).ok().map(|d| kafka_registry::codec::pretty(&d.json));
    Ok((out, bytes))
}

pub(super) async fn try_value(State(s): State<Shared>, body: Bytes) -> Response {
    let Some(r) = s.source.schemas().registry().cloned() else {
        return off();
    };
    let Ok(p) = serde_json::from_slice::<TryPost>(&body) else {
        return bad_body();
    };
    match encode(&r, &p.subject, &p.version, &p.value) {
        Ok((e, _)) => json(StatusCode::OK, &e),
        Err(e) => json(
            StatusCode::OK,
            &Encoded {
                ok: false,
                error: Some(e.message),
                id: 0,
                version: 0,
                kind: String::new(),
                subject: p.subject,
                size: 0,
                hex: String::new(),
                json: None,
            },
        ),
    }
}
