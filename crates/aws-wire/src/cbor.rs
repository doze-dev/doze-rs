//! The Smithy RPC v2 CBOR protocol (`rpcv2Cbor`): a POST to `/service/<Service>/operation/<Operation>` with a CBOR body and the
//! header `smithy-protocol: rpc-v2-cbor`. This is what current SDKs speak to CloudWatch (botocore prefers it to the JSON and Query
//! protocols the service also offers).
//!
//! Decoding is the whole of the new work on the way in: a CBOR body is read into the JSON text the awsJson decoder already checks
//! against the model, so a service sees the same `Call` whichever protocol came in (as the Query codec does). The way out is
//! `aws_schema::cbor`, which writes a service's answer tree as CBOR driven by the schema (a timestamp is tag 1, a blob a byte
//! string). Errors are a CBOR map with `__type` and `message`.
//!
//! The decoder is bounded: nesting is limited as the JSON decoder's is, and lengths are checked against the bytes that are
//! there before anything is allocated for them. It is fuzzed (`fuzz_cbor`).

use aws_schema::meta::ServiceMeta;
use aws_schema::tables::Tables;
use http::request::Parts;
use http::{Response, StatusCode, header};

use crate::json::{Modeled, modeled};
use crate::{ApiError, Fault, RequestId};

/// How deep a body may nest. The same as the JSON decoder's.
const MAX_DEPTH: u32 = 100;

/// Why a body is not CBOR this decoder accepts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum CborError {
    /// The bytes stop in the middle of an item.
    #[error("the body ends inside a CBOR item")]
    Truncated,
    /// A construct this decoder does not read (a map key that is not text, a bad simple value, an indefinite item of the wrong kind).
    #[error("the body holds CBOR that is not accepted here")]
    Unsupported,
    /// Nested deeper than any real request.
    #[error("the body nests too deeply")]
    TooDeep,
    /// A string that is not UTF-8.
    #[error("a text string is not UTF-8")]
    Utf8,
    /// Bytes follow the one item the body should hold.
    #[error("bytes follow the CBOR item")]
    Trailing,
}

/// Whether the request is in this protocol.
#[must_use]
pub fn is_request(parts: &Parts) -> bool {
    parts
        .headers
        .get("smithy-protocol")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v == "rpc-v2-cbor")
}

/// The operation a request asks for: an index into the model's tables.
///
/// # Errors
/// `UnknownOperationException` (404) for a path that does not name this service and one of its operations.
pub fn route(meta: &ServiceMeta, tables: &Tables, parts: &Parts) -> Result<u32, ApiError> {
    let unknown = || ApiError::new(404, "UnknownOperationException", "");
    if parts.method != http::Method::POST {
        return Err(unknown());
    }
    let path = parts.uri.path();
    let rest = path.strip_prefix("/service/").ok_or_else(unknown)?;
    let (service, operation) = rest.split_once("/operation/").ok_or_else(unknown)?;
    if service != meta.shape_name {
        return Err(unknown());
    }
    tables.find_op(operation).ok_or_else(unknown)
}

/// The body as the JSON text of the same value. An empty body is `{}`.
///
/// # Errors
/// [`CborError`] for a body that is not one CBOR item this decoder reads.
// fuzz:parser fuzz_cbor
pub fn to_json(body: &[u8]) -> Result<String, CborError> {
    if body.is_empty() {
        return Ok("{}".to_owned());
    }
    let mut r = Reader { bytes: body, at: 0 };
    let mut out = String::with_capacity(body.len() * 2);
    r.item(&mut out, 0)?;
    if r.at != body.len() {
        return Err(CborError::Trailing);
    }
    Ok(out)
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

/// A CBOR head: the major type, and the argument (`None` for an indefinite length).
struct Head {
    major: u8,
    arg: Option<u64>,
    /// The additional information, which a float or a simple value reads differently.
    info: u8,
}

impl Reader<'_> {
    fn byte(&mut self) -> Result<u8, CborError> {
        let b = *self.bytes.get(self.at).ok_or(CborError::Truncated)?;
        self.at += 1;
        Ok(b)
    }

    fn take(&mut self, n: usize) -> Result<&[u8], CborError> {
        let end = self.at.checked_add(n).ok_or(CborError::Truncated)?;
        let s = self.bytes.get(self.at..end).ok_or(CborError::Truncated)?;
        self.at = end;
        Ok(s)
    }

    fn be(&mut self, n: usize) -> Result<u64, CborError> {
        Ok(self
            .take(n)?
            .iter()
            .fold(0u64, |a, b| (a << 8) | u64::from(*b)))
    }

    fn head(&mut self) -> Result<Head, CborError> {
        let b = self.byte()?;
        let (major, info) = (b >> 5, b & 31);
        let arg = match info {
            0..=23 => Some(u64::from(info)),
            24 => Some(self.be(1)?),
            25 => Some(self.be(2)?),
            26 => Some(self.be(4)?),
            27 => Some(self.be(8)?),
            31 => None,
            _ => return Err(CborError::Unsupported),
        };
        Ok(Head { major, arg, info })
    }

    /// Reads one item and writes it as JSON.
    fn item(&mut self, out: &mut String, depth: u32) -> Result<(), CborError> {
        if depth > MAX_DEPTH {
            return Err(CborError::TooDeep);
        }
        let h = self.head()?;
        match h.major {
            0 => out.push_str(&h.arg.ok_or(CborError::Unsupported)?.to_string()),
            1 => {
                let n = i128::from(h.arg.ok_or(CborError::Unsupported)?);
                out.push_str(&(-1 - n).to_string());
            }
            2 => {
                let bytes = self.string(&h)?;
                out.push('"');
                out.push_str(&aws_core::base64::encode(&bytes));
                out.push('"');
            }
            3 => {
                let bytes = self.string(&h)?;
                let text = std::str::from_utf8(&bytes).map_err(|_| CborError::Utf8)?;
                quote(out, text);
            }
            4 => self.array(&h, out, depth)?,
            5 => self.map(&h, out, depth)?,
            // A tag only says how to read what follows (tag 1 is a timestamp, which the JSON protocol carries as a number).
            6 => self.item(out, depth + 1)?,
            _ => simple(&h, out)?,
        }
        Ok(())
    }

    /// The bytes of a byte or text string, joining the chunks of an indefinite one.
    fn string(&mut self, h: &Head) -> Result<Vec<u8>, CborError> {
        match h.arg {
            Some(n) => Ok(self
                .take(usize::try_from(n).map_err(|_| CborError::Truncated)?)?
                .to_vec()),
            None => {
                let mut all = Vec::new();
                loop {
                    let chunk = self.head()?;
                    if chunk.major == 7 && chunk.info == 31 {
                        return Ok(all);
                    }
                    if chunk.major != h.major || chunk.arg.is_none() {
                        return Err(CborError::Unsupported);
                    }
                    all.extend_from_slice(&self.string(&chunk)?);
                }
            }
        }
    }

    fn array(&mut self, h: &Head, out: &mut String, depth: u32) -> Result<(), CborError> {
        out.push('[');
        let mut first = true;
        let mut left = h.arg;
        loop {
            match left {
                Some(0) => break,
                Some(n) => left = Some(n - 1),
                None if self.bytes.get(self.at) == Some(&0xff) => {
                    self.at += 1;
                    break;
                }
                None => {}
            }
            if !first {
                out.push(',');
            }
            first = false;
            self.item(out, depth + 1)?;
        }
        out.push(']');
        Ok(())
    }

    fn map(&mut self, h: &Head, out: &mut String, depth: u32) -> Result<(), CborError> {
        out.push('{');
        let mut first = true;
        let mut left = h.arg;
        loop {
            match left {
                Some(0) => break,
                Some(n) => left = Some(n - 1),
                None if self.bytes.get(self.at) == Some(&0xff) => {
                    self.at += 1;
                    break;
                }
                None => {}
            }
            if !first {
                out.push(',');
            }
            first = false;
            let key = self.head()?;
            if key.major != 3 {
                return Err(CborError::Unsupported);
            }
            let bytes = self.string(&key)?;
            quote(
                out,
                std::str::from_utf8(&bytes).map_err(|_| CborError::Utf8)?,
            );
            out.push(':');
            self.item(out, depth + 1)?;
        }
        out.push('}');
        Ok(())
    }
}

/// Major type 7: false, true, null, undefined, and the floats.
fn simple(h: &Head, out: &mut String) -> Result<(), CborError> {
    match h.info {
        20 => out.push_str("false"),
        21 => out.push_str("true"),
        22 | 23 => out.push_str("null"),
        25 => float(f64::from(half(h.arg.unwrap_or(0))), out),
        26 => float(
            f64::from(f32::from_bits(
                u32::try_from(h.arg.unwrap_or(0)).unwrap_or(0),
            )),
            out,
        ),
        27 => float(f64::from_bits(h.arg.unwrap_or(0)), out),
        // A break outside an indefinite item, or a simple value with no meaning here.
        _ => return Err(CborError::Unsupported),
    }
    Ok(())
}

/// A number as JSON. NaN and the infinities have no JSON number; the JSON protocols write them as strings.
fn float(f: f64, out: &mut String) {
    if f.is_nan() {
        out.push_str("\"NaN\"");
    } else if f.is_infinite() {
        out.push_str(if f > 0.0 {
            "\"Infinity\""
        } else {
            "\"-Infinity\""
        });
    } else {
        out.push_str(&format!("{f:?}"));
    }
}

/// An IEEE 754 half-precision float as a single.
fn half(bits: u64) -> f32 {
    let b = u32::try_from(bits).unwrap_or(0);
    let (sign, exp, frac) = ((b >> 15) & 1, (b >> 10) & 31, b & 1023);
    let magnitude = match exp {
        0 => f32::from(u16::try_from(frac).unwrap_or(0)) * 2f32.powi(-24),
        31 if frac == 0 => f32::INFINITY,
        31 => f32::NAN,
        e => {
            (1.0 + f32::from(u16::try_from(frac).unwrap_or(0)) / 1024.0)
                * 2f32.powi(i32::try_from(e).unwrap_or(15) - 15)
        }
    };
    if sign == 1 { -magnitude } else { magnitude }
}

fn quote(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// The 200 answer for a body already encoded.
#[must_use]
pub fn success(id: &RequestId, body: Vec<u8>) -> Response<Vec<u8>> {
    base(StatusCode::OK, id).body(body).unwrap_or_default()
}

fn base(status: StatusCode, id: &RequestId) -> http::response::Builder {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/cbor")
        .header("smithy-protocol", "rpc-v2-cbor")
        .header("x-amzn-RequestId", id.as_str())
}

/// The answer for `err`: the status and code from the model when the error is modelled, else as the error states them. The body is
/// a CBOR map `{"__type": ..., "message": ...}`; a query-compatible service also says the code the Query protocol would.
#[must_use]
pub fn failure(
    meta: &ServiceMeta,
    tables: &Tables,
    id: &RequestId,
    err: &ApiError,
    flat_client_errors: bool,
) -> Response<Vec<u8>> {
    let found = err.shape.as_deref().and_then(|name| modeled(tables, name));
    let (status, fault, wire_code, query_code) = match &found {
        Some(Modeled {
            name,
            query_code,
            status,
            fault,
        }) => (
            if flat_client_errors && *status < 500 {
                400
            } else {
                *status
            },
            *fault,
            name.as_str(),
            Some(query_code.as_str()),
        ),
        None => (err.status, err.fault, err.code.as_ref(), None),
    };
    let namespace = if found.is_some() {
        meta.namespace
    } else {
        "com.amazon.coral.service"
    };
    let mut builder = base(
        StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_REQUEST),
        id,
    );
    if err.status == 501 && err.code == "NotImplemented" {
        builder = builder.header("x-doze-not-built", "1");
    }
    if meta.query_compatible {
        let who = if fault == Fault::Receiver {
            "Receiver"
        } else {
            "Sender"
        };
        builder = builder.header(
            "x-amzn-query-error",
            format!("{};{who}", query_code.unwrap_or(err.code.as_ref())),
        );
    }
    let mut body = Vec::with_capacity(48 + err.message.len());
    aws_schema::cbor::map_header(&mut body, 2);
    aws_schema::cbor::text(&mut body, "__type");
    aws_schema::cbor::text(&mut body, &format!("{namespace}#{wire_code}"));
    aws_schema::cbor::text(&mut body, "message");
    aws_schema::cbor::text(&mut body, &err.message);
    builder.body(body).unwrap_or_default()
}

/// The answer for an error raised before any service is known.
#[must_use]
pub fn generic_failure(id: &RequestId, err: &ApiError) -> Response<Vec<u8>> {
    let mut body = Vec::with_capacity(48 + err.message.len());
    aws_schema::cbor::map_header(&mut body, 2);
    aws_schema::cbor::text(&mut body, "__type");
    aws_schema::cbor::text(&mut body, &format!("com.amazon.coral.service#{}", err.code));
    aws_schema::cbor::text(&mut body, "message");
    aws_schema::cbor::text(&mut body, &err.message);
    base(
        StatusCode::from_u16(err.status).unwrap_or(StatusCode::BAD_REQUEST),
        id,
    )
    .body(body)
    .unwrap_or_default()
}

#[cfg(test)]
mod tests;
