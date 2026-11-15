//! Getting a function's (or a layer's) code in, as AWS takes it: an inline archive, an object in the local S3 (read over the peer bus
//! as the caller), or an image URI (stored, never run). An archive is written whole as `code.zip` and unpacked beside it, in a directory named by a fresh id; the directory is
//! never changed afterwards, so a published version can share it with `$LATEST`.

use std::path::{Path, PathBuf};

use aws_core::base64;
use aws_peers::{Need, Peers};
use aws_wire::ApiError;
use aws_wire::service::Caller;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::errors::{invalid, not_found};
use crate::model::Code;
use crate::zipread::{self, MAX_UNPACKED, ZipError};

/// The most a zipped package may be (Lambda: 50 MB direct, up to the unzipped limit through S3).
const MAX_ZIPPED: usize = 262_144_000;

/// The unpacked tree of `code`, which a process runs from.
pub(crate) fn root(base: &Path, code: &Code) -> PathBuf {
    match &code.local {
        // A record from before 2026-11-15, when a directory could be named as code: it still runs from there.
        Some(path) => PathBuf::from(path),
        None => base.join("code").join(&code.id).join("root"),
    }
}

fn unzip_error(e: ZipError) -> ApiError {
    match e {
        ZipError::Malformed => invalid(
            "Could not unzip uploaded file. Please check your file, then try to upload again.",
        ),
        ZipError::TooBig => invalid(format!(
            "Unzipped size must be smaller than {MAX_UNPACKED} bytes"
        )),
        ZipError::Io => ApiError::internal("the package could not be written"),
    }
}

/// Reads the `Code` (or a layer's `Content`) member into a stored [`Code`], writing and unpacking an archive.
///
/// # Errors
/// A package that is not a zip, is too big, names an object that is not there, or names no source.
pub(crate) async fn stage(
    base: &Path,
    peers: Option<&Peers>,
    caller: &Caller,
    spec: &Value,
) -> Result<Code, ApiError> {
    let text = |k: &str| {
        spec.get(k)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
    };
    if let Some(image) = text("ImageUri") {
        return Ok(Code {
            id: String::new(),
            local: None,
            sha256: String::new(),
            size: 0,
            image: Some(image.to_owned()),
        });
    }
    let bytes = if let Some(zip) = text("ZipFile") {
        base64::decode(zip).ok_or_else(|| invalid("Could not decode the ZipFile."))?
    } else if let (Some(bucket), Some(key)) = (text("S3Bucket"), text("S3Key")) {
        fetch(peers, caller, bucket, key, text("S3ObjectVersion")).await?
    } else {
        return Err(invalid(
            "Please provide a source for function code. Zip file, S3 bucket and key, or an image URI.",
        ));
    };
    unpack(base, bytes).await
}

async fn unpack(base: &Path, bytes: Vec<u8>) -> Result<Code, ApiError> {
    if bytes.len() > MAX_ZIPPED {
        return Err(invalid("Zipped size must be smaller than 262144000 bytes"));
    }
    let id = aws_seal::uuid_v4().ok_or_else(|| ApiError::internal("no randomness"))?;
    let dir = base.join("code").join(&id);
    let sha256 = base64::encode(&Sha256::digest(&bytes));
    let size = bytes.len() as u64;
    let unpacked = tokio::task::spawn_blocking(move || {
        let made = zipread::extract(&bytes, &dir.join("root"));
        if made.is_ok() {
            std::fs::write(dir.join("code.zip"), &bytes).map_err(|_| ZipError::Io)?;
        } else {
            drop(std::fs::remove_dir_all(&dir));
        }
        made
    })
    .await
    .map_err(|_| ApiError::internal("unpacking was interrupted"))?;
    unpacked.map_err(unzip_error)?;
    Ok(Code {
        id,
        local: None,
        sha256,
        size,
        image: None,
    })
}

/// The object from the local S3, read as the caller (their `s3:GetObject` is asked for first).
async fn fetch(
    peers: Option<&Peers>,
    caller: &Caller,
    bucket: &str,
    key: &str,
    version: Option<&str>,
) -> Result<Vec<u8>, ApiError> {
    let peers =
        peers.ok_or_else(|| invalid("S3 is not running, so the code cannot be read from it."))?;
    peers
        .authorize(
            caller,
            vec![Need::new(
                "s3:GetObject",
                format!("arn:aws:s3:::{bucket}/{key}"),
            )],
        )
        .await?;
    let mut body = json!({"Bucket": bucket, "Key": key});
    if let Some(v) = version {
        crate::set(&mut body, "VersionId", json!(v));
    }
    let mut reply = peers
        .call_rest("lambda", caller, "s3", "GetObject", &body)
        .await
        .map_err(|e| {
            if e.status == 404 {
                invalid(format!(
                    "Error occurred while GetObject. S3 Error Code: {}. S3 Error Message: {}",
                    e.code, e.message
                ))
            } else {
                e
            }
        })?;
    let Some(mut stream) = reply.body.take() else {
        return Err(not_found("The object has no body."));
    };
    let mut out = Vec::new();
    while let Some(chunk) = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await {
        let chunk = chunk.map_err(|e| ApiError::internal(format!("reading the object: {e}")))?;
        out.extend_from_slice(&chunk);
        if out.len() > MAX_ZIPPED {
            return Err(invalid("Zipped size must be smaller than 262144000 bytes"));
        }
    }
    Ok(out)
}

/// Removes the directories of `code/` that none of `keep` names. Called after anything that drops a reference.
pub(crate) fn sweep(base: &Path, keep: &std::collections::BTreeSet<String>) {
    let Ok(read) = std::fs::read_dir(base.join("code")) else {
        return;
    };
    for entry in read.filter_map(Result::ok) {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !keep.contains(&name) {
            drop(std::fs::remove_dir_all(entry.path()));
        }
    }
}

impl crate::Inner {
    /// What an operation needs read before its state is touched: the code it brings, and the role it names.
    pub(crate) async fn stage(
        &self,
        op: aws_model_lambda::Operation,
        r: &crate::req::Req,
    ) -> Result<crate::core::Staged, ApiError> {
        use aws_model_lambda::Operation;
        let mut staged = crate::core::Staged::default();
        let base = self.region_dir(r.region());
        let spec = match op {
            Operation::CreateFunction => r.value("Code"),
            Operation::PublishLayerVersion => r.value("Content"),
            Operation::UpdateFunctionCode if r.flag("DryRun") != Some(true) => Some(&r.body),
            _ => None,
        };
        if let Some(spec) = spec {
            staged.code = Some(stage(&base, self.peers.as_deref(), &r.caller, spec).await?);
        }
        if matches!(
            op,
            Operation::CreateFunction | Operation::UpdateFunctionConfiguration
        ) && let Some(role) = r.opt("Role")
        {
            staged.role = crate::role::check(self.peers.as_deref(), &r.caller, role).await;
        }
        Ok(staged)
    }
}
