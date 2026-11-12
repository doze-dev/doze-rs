//! An object's bytes arriving: the upload is written to a staging file as it comes, summed on the way, and checked against what the
//! client declared (the length, `Content-MD5`, a checksum in a header or a trailer). Nothing is held but the piece in hand.

use aws_wire::ApiError;
use aws_wire::rest::Upload;
use doze_fs::fs::{FsFile, RealFs};
use serde_json::Value;

use crate::blob::{Blobs, Staging};
use crate::checksum::{Algo, Done, Sums, b64};
use crate::errors::{bad_digest, invalid_request};
use crate::req::Staged;

/// What the request declared about its body.
pub(crate) struct Declared {
    /// Algorithms to compute: the one named and each one given a value, or CRC64NVME when none is.
    pub(crate) want: Vec<Algo>,
    /// (algorithm, base64 value) the client gave in headers.
    pub(crate) values: Vec<(Algo, String)>,
    pub(crate) md5: Option<String>,
    pub(crate) sha256_hex: Option<String>,
}

impl Declared {
    /// Reads the declarations out of the request's JSON.
    pub(crate) fn of(v: &Value, x_amz_content_sha256: Option<&str>) -> Self {
        let mut want = Vec::new();
        let mut values = Vec::new();
        if let Some(a) = v.get("ChecksumAlgorithm").and_then(Value::as_str).and_then(Algo::parse) {
            want.push(a);
        }
        for a in Algo::ALL {
            if let Some(s) = v.get(a.member()).and_then(Value::as_str) {
                values.push((a, s.to_owned()));
                if !want.contains(&a) {
                    want.push(a);
                }
            }
        }
        if want.is_empty() {
            want.push(Algo::Crc64Nvme);
        }
        Self {
            want,
            values,
            md5: v.get("ContentMD5").and_then(Value::as_str).map(str::to_owned),
            sha256_hex: x_amz_content_sha256
                .filter(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()))
                .map(str::to_ascii_lowercase),
        }
    }
}

fn read_failed(e: &std::io::Error) -> ApiError {
    ApiError::internal(format!("the object could not be stored: {e}"))
}

/// Writes `data` at `at` on a blocking thread.
async fn write_at(file: std::sync::Arc<<RealFs as doze_fs::fs::Fs>::File>, data: bytes::Bytes, at: u64) -> Result<(), ApiError> {
    tokio::task::spawn_blocking(move || file.write_all_at(&data, at))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| read_failed(&e))
}

/// Reads `upload` to its end into a new staging file.
///
/// # Errors
/// The stream broke, the disk refused, or a declared length, MD5 or checksum does not match what arrived. The staging file is
/// removed.
pub(crate) async fn receive(
    blobs: &Blobs<RealFs>,
    upload: &mut dyn Upload,
    declared: &Declared,
) -> Result<Staged, ApiError> {
    let mut staging = blobs.begin().map_err(|e| read_failed(&e))?;
    let result = fill(&mut staging, upload, declared).await;
    match result {
        Ok(done) => Ok(Staged { staging, done }),
        Err(e) => {
            blobs.discard(&staging);
            Err(e)
        }
    }
}

async fn fill(staging: &mut Staging<RealFs>, upload: &mut dyn Upload, d: &Declared) -> Result<Done, ApiError> {
    let mut want = d.want.clone();
    if d.sha256_hex.is_some() && !want.contains(&Algo::Sha256) {
        want.push(Algo::Sha256);
    }
    let mut sums = Sums::new(&want);
    while let Some(piece) = upload.next().await? {
        sums.update(&piece);
        let n = piece.len() as u64;
        write_at(staging.file(), piece, staging.len).await?;
        staging.len += n;
    }
    if let Some(declared) = upload.length().filter(|l| *l != staging.len) {
        return Err(invalid_request(&format!(
            "The request body is {} bytes but x-amz-decoded-content-length said {declared}.",
            staging.len
        )));
    }
    let done = sums.finish();
    verify(&done, d, &upload.trailers())?;
    Ok(done)
}

/// Checks the sums against everything the client declared.
fn verify(done: &Done, d: &Declared, trailers: &[(String, String)]) -> Result<(), ApiError> {
    if let Some(want) = &d.md5
        && b64(&done.md5) != *want
    {
        return Err(bad_digest("Content-MD5"));
    }
    if let Some(want) = &d.sha256_hex {
        let got = done
            .sums
            .iter()
            .find(|(a, _)| *a == Algo::Sha256)
            .map(|(_, raw)| aws_core::hex::encode(raw));
        if got.as_deref() != Some(want.as_str()) {
            return Err(ApiError::new(
                400,
                "XAmzContentSHA256Mismatch",
                "The provided 'x-amz-content-sha256' header does not match what was computed.",
            ));
        }
    }
    let from_trailers = trailers.iter().filter_map(|(k, v)| {
        Algo::ALL.into_iter().find(|a| a.header() == k).map(|a| (a, v.clone()))
    });
    for (algo, want) in d.values.iter().cloned().chain(from_trailers) {
        if let Some((_, raw)) = done.sums.iter().find(|(a, _)| *a == algo)
            && b64(raw) != want
        {
            return Err(bad_digest(algo.name()));
        }
    }
    Ok(())
}
