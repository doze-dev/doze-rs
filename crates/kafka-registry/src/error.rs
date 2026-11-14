//! A registry failure as Confluent's REST API reports it: an HTTP status, and Confluent's error code and sentence in the
//! body. The codes are Confluent's because every serializer keys on them (40401 means "no such subject" to all of them);
//! the sentences are Confluent's word for word where a recording holds them (`tests/data/conformance`).

use serde::Serialize;

/// Confluent's error codes.
pub mod code {
    pub const SUBJECT_NOT_FOUND: u32 = 40401;
    pub const VERSION_NOT_FOUND: u32 = 40402;
    pub const SCHEMA_NOT_FOUND: u32 = 40403;
    pub const SUBJECT_SOFT_DELETED: u32 = 40404;
    pub const SUBJECT_NOT_SOFT_DELETED: u32 = 40405;
    pub const VERSION_SOFT_DELETED: u32 = 40406;
    pub const VERSION_NOT_SOFT_DELETED: u32 = 40407;
    pub const SUBJECT_COMPAT_NOT_SET: u32 = 40408;
    pub const SUBJECT_MODE_NOT_SET: u32 = 40409;
    pub const INCOMPATIBLE: u32 = 409;
    pub const INVALID_SCHEMA: u32 = 42201;
    pub const INVALID_VERSION: u32 = 42202;
    pub const INVALID_COMPATIBILITY: u32 = 42203;
    pub const INVALID_MODE: u32 = 42204;
    pub const OPERATION_NOT_PERMITTED: u32 = 42205;
    pub const REFERENCE_EXISTS: u32 = 42206;
    pub const INVALID_REQUEST: u32 = 42207;
    pub const INVALID_SUBJECT: u32 = 42208;
    pub const STORE: u32 = 50001;
}

/// One failure: the HTTP status it is answered with, and the body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error)]
#[error("{error_code}: {message}")]
pub struct RegistryError {
    #[serde(skip)]
    pub status: u16,
    pub error_code: u32,
    pub message: String,
}

impl RegistryError {
    #[must_use]
    pub fn new(status: u16, error_code: u32, message: impl Into<String>) -> Self {
        Self {
            status,
            error_code,
            message: message.into(),
        }
    }

    /// An HTTP-level refusal whose code is its status (401, 403, 404, 405, 413, 415), as Confluent's server answers them.
    #[must_use]
    pub fn http(status: u16, message: impl Into<String>) -> Self {
        Self::new(status, u32::from(status), message)
    }

    #[must_use]
    pub fn subject_not_found(subject: &str) -> Self {
        Self::new(
            404,
            code::SUBJECT_NOT_FOUND,
            format!("Subject '{subject}' not found."),
        )
    }

    #[must_use]
    pub fn version_not_found(version: u32) -> Self {
        Self::new(
            404,
            code::VERSION_NOT_FOUND,
            format!("Version {version} not found."),
        )
    }

    #[must_use]
    pub fn schema_not_found() -> Self {
        Self::new(404, code::SCHEMA_NOT_FOUND, "Schema not found")
    }

    #[must_use]
    pub fn schema_id_not_found(id: u32) -> Self {
        Self::new(
            404,
            code::SCHEMA_NOT_FOUND,
            format!("Schema {id} not found"),
        )
    }

    #[must_use]
    pub fn invalid_schema(message: impl Into<String>) -> Self {
        Self::new(422, code::INVALID_SCHEMA, message)
    }

    #[must_use]
    pub fn invalid_version(text: &str) -> Self {
        Self::new(
            422,
            code::INVALID_VERSION,
            format!(
                "The specified version '{text}' is not a valid version id. Allowed values are between [1, 2^31-1] and the string \"latest\""
            ),
        )
    }

    #[must_use]
    pub fn invalid_compatibility() -> Self {
        Self::new(
            422,
            code::INVALID_COMPATIBILITY,
            "Invalid compatibility level. Valid values are none, backward, forward, full, backward_transitive, forward_transitive, and full_transitive",
        )
    }

    #[must_use]
    pub fn invalid_mode() -> Self {
        Self::new(
            422,
            code::INVALID_MODE,
            "Invalid mode. Valid values are READWRITE and READONLY",
        )
    }

    #[must_use]
    pub fn read_only(subject: &str) -> Self {
        Self::new(
            422,
            code::OPERATION_NOT_PERMITTED,
            format!("Subject {subject} is in read-only mode"),
        )
    }

    #[must_use]
    pub fn reference_exists(subject: &str, version: u32) -> Self {
        Self::new(
            422,
            code::REFERENCE_EXISTS,
            format!(
                "One or more references exist to the schema {{magic=1,keytype=SCHEMA,subject={subject},version={version}}}."
            ),
        )
    }

    #[must_use]
    pub fn store(why: impl std::fmt::Display) -> Self {
        Self::new(
            500,
            code::STORE,
            format!("Error while committing to the schema store: {why}"),
        )
    }
}
