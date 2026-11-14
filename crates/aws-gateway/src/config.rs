//! How the gateway is set up.

use std::net::SocketAddr;
use std::sync::Arc;

use aws_auth::{CredentialSource, StaticCredentials};

/// Who may call.
#[derive(Clone, Debug)]
pub enum Auth {
    /// Route by the credential scope and accept any access key without checking the signature. For a local development
    /// tool whose callers use whatever keys their environment has; every caller is the local account.
    Trust,
    /// Check every signature against these credentials.
    Verify(Arc<StaticCredentials>),
}

/// The gateway's settings.
#[derive(Clone, Debug)]
pub struct Config {
    /// Where to listen. Port 0 picks one.
    pub listen: SocketAddr,
    /// Who may call.
    pub auth: Auth,
    /// Where to look for a key the fixed credentials of [`Auth::Verify`] do not hold (IAM's access keys and role sessions).
    pub credential_source: Option<Arc<dyn CredentialSource>>,
    /// The account every request is for until IAM says otherwise.
    pub account_id: String,
    /// The region a request that names none is for: an unsigned S3 request (`curl`, a browser) and a SigV2 presigned URL. AWS
    /// sends both to `us-east-1` (the global S3 endpoint), the default.
    pub default_region: String,
    /// The facts `GET /_doze/health` answers with (the gateway adds `uptime_secs`). `None` leaves the path to the services.
    pub health: Option<serde_json::Map<String, serde_json::Value>>,
    /// The most request body read into memory, in bytes.
    pub max_body_bytes: usize,
    /// How far a client's clock may differ from ours, in seconds.
    pub max_skew_secs: i64,
}

impl Config {
    /// The development defaults, listening on `listen`.
    #[must_use]
    pub fn new(listen: SocketAddr) -> Self {
        Self {
            listen,
            auth: Auth::Trust,
            credential_source: None,
            account_id: "000000000000".to_owned(),
            default_region: "us-east-1".to_owned(),
            health: None,
            max_body_bytes: 16 << 20,
            max_skew_secs: 300,
        }
    }
}
