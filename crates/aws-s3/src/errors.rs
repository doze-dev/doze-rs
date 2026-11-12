//! The errors S3 answers with, worded as the recordings show them. A message marked `(memory)` was not in a recording.

use aws_wire::ApiError;

fn e(status: u16, code: &'static str, message: &str) -> ApiError {
    ApiError::new(status, code, message)
}

pub(crate) fn no_such_bucket(bucket: &str) -> ApiError {
    e(404, "NoSuchBucket", "The specified bucket does not exist").with("BucketName", bucket)
}

pub(crate) fn no_such_key(key: &str) -> ApiError {
    e(404, "NoSuchKey", "The specified key does not exist.").with("Key", key)
}

pub(crate) fn no_such_version(key: &str, version: &str) -> ApiError {
    // (memory) AWS words an absent version of an existing key this way.
    e(
        404,
        "NoSuchVersion",
        "The specified version does not exist.",
    )
    .with("Key", key)
    .with("VersionId", version)
}

pub(crate) fn no_such_upload(id: &str) -> ApiError {
    e(
        404,
        "NoSuchUpload",
        "The specified upload does not exist. The upload ID may be invalid, or the upload may have been aborted or completed.",
    )
    .with("UploadId", id)
}

pub(crate) fn bucket_not_empty(bucket: &str) -> ApiError {
    e(
        409,
        "BucketNotEmpty",
        "The bucket you tried to delete is not empty",
    )
    .with("BucketName", bucket)
}

pub(crate) fn invalid_bucket_name(bucket: &str) -> ApiError {
    e(
        400,
        "InvalidBucketName",
        "The specified bucket is not valid.",
    )
    .with("BucketName", bucket)
}

pub(crate) fn already_owned(bucket: &str) -> ApiError {
    e(
        409,
        "BucketAlreadyOwnedByYou",
        "Your previous request to create the named bucket succeeded and you already own it.",
    )
    .with("BucketName", bucket)
}

pub(crate) fn precondition_failed(condition: &str) -> ApiError {
    e(
        412,
        "PreconditionFailed",
        "At least one of the pre-conditions you specified did not hold",
    )
    .with("Condition", condition)
}

pub(crate) fn invalid_range(size: u64, requested: &str) -> ApiError {
    e(
        416,
        "InvalidRange",
        "The requested range is not satisfiable",
    )
    .with("ActualObjectSize", size.to_string())
    .with("RangeRequested", requested)
}

pub(crate) fn invalid_argument(message: &str) -> ApiError {
    e(400, "InvalidArgument", message)
}

pub(crate) fn invalid_request(message: &str) -> ApiError {
    e(400, "InvalidRequest", message)
}

pub(crate) fn malformed_xml() -> ApiError {
    e(
        400,
        "MalformedXML",
        "The XML you provided was not well-formed or did not validate against our published schema",
    )
}

pub(crate) fn bad_digest(what: &str) -> ApiError {
    e(
        400,
        "BadDigest",
        &format!("The {what} you specified did not match the calculated checksum."),
    )
}

pub(crate) fn not_modified() -> ApiError {
    e(304, "NotModified", "Not Modified")
}

pub(crate) fn key_too_long() -> ApiError {
    e(400, "KeyTooLongError", "Your key is too long")
}

/// A configuration that is not there: the code and words differ for each kind (recorded).
pub(crate) fn no_config(kind: &str) -> ApiError {
    let (code, message) = match kind {
        "cors" => (
            "NoSuchCORSConfiguration",
            "The CORS configuration does not exist",
        ),
        "lifecycle" => (
            "NoSuchLifecycleConfiguration",
            "The lifecycle configuration does not exist",
        ),
        "policy" => ("NoSuchBucketPolicy", "The bucket policy does not exist"),
        "website" => (
            "NoSuchWebsiteConfiguration",
            "The specified bucket does not have a website configuration",
        ),
        "tagging" => ("NoSuchTagSet", "The TagSet does not exist"),
        // (memory) these three are worded from the documented codes.
        "encryption" => (
            "ServerSideEncryptionConfigurationNotFoundError",
            "The server side encryption configuration was not found",
        ),
        "ownership" => (
            "OwnershipControlsNotFoundError",
            "The bucket ownership controls were not found",
        ),
        "public-access-block" => (
            "NoSuchPublicAccessBlockConfiguration",
            "The public access block configuration was not found",
        ),
        "replication" => (
            "ReplicationConfigurationNotFoundError",
            "The replication configuration was not found",
        ),
        "object-lock" => (
            "ObjectLockConfigurationNotFoundError",
            "Object Lock configuration does not exist for this bucket",
        ),
        _ => ("NoSuchConfiguration", "The configuration does not exist"),
    };
    e(404, code, message)
}

/// The policy documents S3 refuses (recorded wording).
pub(crate) fn malformed_policy(message: &str) -> ApiError {
    e(400, "MalformedPolicy", message)
}

pub(crate) fn invalid_bucket_state(message: &str) -> ApiError {
    e(409, "InvalidBucketState", message)
}
