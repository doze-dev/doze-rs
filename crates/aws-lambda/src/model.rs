//! What Lambda keeps. A function's configuration is kept as the JSON an API caller set (`Runtime`, `Role`, `Environment`, ...), merged
//! on update and rendered with its defaults on read: one representation, so a member the API adds needs no new field here.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Where a function's code is.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Code {
    /// The directory under `code/` holding `code.zip` and the unpacked `root/`; empty for code run in place or an image.
    pub(crate) id: String,
    /// An absolute path code runs from in place (`S3Bucket: _local_`), instead of an unpacked archive.
    pub(crate) local: Option<String>,
    /// The base64 SHA-256 of the archive (a fingerprint of the tree for code in place).
    pub(crate) sha256: String,
    pub(crate) size: u64,
    /// The image URI of a container-image function: stored, never run.
    pub(crate) image: Option<String>,
}

/// One state of a function: the configuration and the code, as of a time.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Snap {
    /// The configuration members as set, JSON text.
    pub(crate) config: String,
    pub(crate) code: Code,
    pub(crate) revision: String,
    pub(crate) modified_ms: i64,
}

/// A published version.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Published {
    pub(crate) number: u32,
    pub(crate) snap: Snap,
}

/// An alias.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Alias {
    pub(crate) name: String,
    pub(crate) version: String,
    pub(crate) description: String,
    /// `AdditionalVersionWeights` as JSON text (`{"2":0.1}`), when the alias splits traffic.
    pub(crate) weights: Option<String>,
    pub(crate) revision: String,
}

/// A function URL.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Url {
    /// The alias the URL is on; empty for `$LATEST`.
    pub(crate) qualifier: String,
    pub(crate) id: String,
    pub(crate) auth: String,
    pub(crate) invoke_mode: String,
    pub(crate) cors: Option<String>,
    pub(crate) created: String,
    pub(crate) modified: String,
}

/// A function with everything that hangs from it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Function {
    pub(crate) name: String,
    pub(crate) latest: Snap,
    pub(crate) versions: Vec<Published>,
    pub(crate) next_version: u32,
    pub(crate) aliases: Vec<Alias>,
    pub(crate) urls: Vec<Url>,
    /// The resource policy document (JSON text) of each ARN form, keyed by qualifier (empty for the unqualified ARN).
    pub(crate) policies: BTreeMap<String, String>,
    /// Asynchronous invocation settings (JSON text) keyed by qualifier (`$LATEST` for the unqualified).
    pub(crate) event_invoke: BTreeMap<String, String>,
    /// Provisioned concurrency settings (JSON text) keyed by qualifier: accepted and flagged, nothing is pre-warmed.
    pub(crate) provisioned: BTreeMap<String, String>,
    pub(crate) reserved: Option<i64>,
    pub(crate) tags: BTreeMap<String, String>,
    /// Settings accepted and stored as sent (runtime management, recursion, code signing).
    pub(crate) extras: BTreeMap<String, String>,
}

/// A version of a layer.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct LayerVersion {
    pub(crate) number: u32,
    pub(crate) description: String,
    pub(crate) runtimes: Vec<String>,
    pub(crate) architectures: Vec<String>,
    pub(crate) license: Option<String>,
    pub(crate) code: Code,
    pub(crate) created_ms: i64,
    /// The layer version's permission statements (JSON text), by statement id.
    pub(crate) statements: BTreeMap<String, String>,
}

/// A layer.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Layer {
    pub(crate) name: String,
    pub(crate) versions: Vec<LayerVersion>,
    pub(crate) next_version: u32,
}

/// An event source mapping: the API object as JSON text, with the state the poller keeps beside it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Mapping {
    pub(crate) uuid: String,
    /// The mapping as the API shows it (`FunctionArn`, `EventSourceArn`, `BatchSize`, ...), JSON text.
    pub(crate) config: String,
    pub(crate) enabled: bool,
    pub(crate) modified_ms: i64,
    /// What the last invocation did, shown as `LastProcessingResult`.
    pub(crate) last_result: Option<String>,
    pub(crate) tags: BTreeMap<String, String>,
}

/// A code signing configuration: accepted and stored, never enforced.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Signing {
    pub(crate) arn: String,
    pub(crate) id: String,
    pub(crate) config: String,
    pub(crate) modified: String,
}
