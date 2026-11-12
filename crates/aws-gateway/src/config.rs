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
            max_body_bytes: 16 << 20,
            max_skew_secs: 300,
        }
    }
}
