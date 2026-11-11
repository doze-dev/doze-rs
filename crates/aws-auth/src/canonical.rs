//! Canonical request and string-to-sign (SigV4).
use crate::{error::AuthError, key};
use aws_core::uri;
use http::{HeaderMap, request::Parts};

/// Inputs that differ per service family.
#[derive(Debug, Clone, Copy)]
pub struct Flavor {
    /// S3: do not normalise the path, encode it once.
    pub s3: bool,
    /// Presigned: the query carries the signature, which is excluded from itself.
    pub presigned: bool,
}

/// Build the canonical request for `parts`.
///
/// # Errors
/// [`AuthError::MissingSignedHeader`] when the client signed a header it did not send.
pub fn canonical_request(
    parts: &Parts,
    signed_headers: &[String],
    payload_hash: &str,
    flavor: Flavor,
) -> Result<String, AuthError> {
    let mut out = String::with_capacity(256);
    out.push_str(parts.method.as_str());
    out.push('\n');
    canonical_path(&mut out, parts.uri.path(), flavor.s3);
    out.push('\n');
    canonical_query(&mut out, parts.uri.query().unwrap_or(""), flavor.presigned);
    out.push('\n');
    for name in signed_headers {
        out.push_str(name);
        out.push(':');
        append_header_value(&mut out, &parts.headers, name)?;
        out.push('\n');
    }
    out.push('\n');
    for (i, name) in signed_headers.iter().enumerate() {
        if i > 0 {
            out.push(';');
        }
        out.push_str(name);
    }
    out.push('\n');
    out.push_str(payload_hash);
    Ok(out)
}

fn canonical_path(out: &mut String, raw: &str, s3: bool) {
    if s3 {
        // S3 signs the path once-encoded and never normalises it. Re-encode the
        // decoded form so clients that over- or under-escape (`!`, `*`) agree.
        uri::encode_into(out, &uri::decode(raw), true);
    } else {
        // Everyone else encodes the already-encoded wire path a second time.
        uri::encode_into(out, uri::normalize_path(raw).as_bytes(), true);
    }
}

fn canonical_query(out: &mut String, query: &str, presigned: bool) {
    let mut pairs: Vec<(String, String)> = form_urlencoded::parse(query.as_bytes())
        .filter(|(k, _)| !(presigned && k == "X-Amz-Signature"))
        .map(|(k, v)| {
            (
                uri::encode(k.as_bytes(), false),
                uri::encode(v.as_bytes(), false),
            )
        })
        .collect();
    pairs.sort_unstable();
    for (i, (k, v)) in pairs.iter().enumerate() {
        if i > 0 {
            out.push('&');
        }
        out.push_str(k);
        out.push('=');
        out.push_str(v);
    }
}

fn append_header_value(out: &mut String, headers: &HeaderMap, name: &str) -> Result<(), AuthError> {
    let mut first = true;
    for v in headers.get_all(name) {
        if !first {
            out.push(',');
        }
        first = false;
        // Trim and collapse runs of spaces/tabs into one space.
        let mut prev_space = true;
        let start = out.len();
        // Non-ASCII values (UTF-8 metadata) are signed as their UTF-8 bytes.
        for c in String::from_utf8_lossy(v.as_bytes()).chars() {
            if c == ' ' || c == '\t' {
                if !prev_space {
                    out.push(' ');
                }
                prev_space = true;
            } else {
                out.push(c);
                prev_space = false;
            }
        }
        if out.len() > start && out.ends_with(' ') {
            out.pop();
        }
    }
    if first {
        return Err(AuthError::MissingSignedHeader(name.to_owned()));
    }
    Ok(())
}

/// `AWS4-HMAC-SHA256\n<date>\n<scope>\n<hex sha256(canonical)>`.
#[must_use]
pub fn string_to_sign(amz_date: &str, scope: &str, canonical: &str) -> String {
    format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
        key::sha256_hex(canonical.as_bytes())
    )
}
