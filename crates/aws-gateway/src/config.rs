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
    /// Under [`Auth::Trust`], the keys whose secret is known anyway (the configured key). A signature on an address a service owns
    /// (a function URL with `AWS_IAM`) made with one of them, or with a key the credential source issued, is checked; one made with
    /// any other key is taken on trust once its scope and time are checked.
    pub known_keys: Arc<StaticCredentials>,
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
    /// Whether a verifying gateway serves an unsigned S3 request (as AWS does) as the anonymous principal, for IAM to judge. Off,
    /// such a request is refused as unauthenticated. A trusting gateway always serves it.
    pub anonymous: bool,
    /// Local hooks of the host's own (`/_doze/data`), asked before any service's addresses. Not AWS addresses: each owns a path
    /// under `/_doze/`, which no bucket name can start with.
    pub hooks: Hooks,
    /// The secret key a request to a service's local address (`/_doze/iam/...`, [`aws_wire::service::Web::local`]) must carry in
    /// `X-Doze-Key`: set under `--auth verify`, `None` when signatures are taken on trust.
    pub local_key: Option<String>,
    /// Told about every finished AWS API request (the console's Live ring). `None`, the default, costs a request nothing.
    pub observer: Option<Arc<dyn crate::Observer>>,
    /// With an observer: the requests in flight, which IAM's let-through refusals under `--iam soft` are marked on
    /// ([`crate::Finished::would_be_denied`]) and which a call between services is matched to as its cause
    /// ([`crate::WouldDeny::request_of`]). `None`, the default, costs a request nothing.
    pub would_deny: Option<Arc<crate::WouldDeny>>,
    /// The Lab's fault rules: asked about each request after it is decoded and before its service is, which may make it wait, fail
    /// it with an error of the service's own, or close the connection ([`crate::fault`]). `None`, the default, costs a request one
    /// `Option` test.
    pub faults: Option<Arc<dyn crate::Faults>>,
}

impl Config {
    /// The development defaults, listening on `listen`.
    #[must_use]
    pub fn new(listen: SocketAddr) -> Self {
        Self {
            listen,
            auth: Auth::Trust,
            credential_source: None,
            known_keys: Arc::default(),
            account_id: "000000000000".to_owned(),
            default_region: "us-east-1".to_owned(),
            health: None,
            max_body_bytes: 16 << 20,
            max_skew_secs: 300,
            anonymous: false,
            hooks: Hooks::default(),
            local_key: None,
            observer: None,
            would_deny: None,
            faults: None,
        }
    }
}

/// The host's local hooks ([`Config::hooks`]).
#[derive(Clone, Default)]
pub struct Hooks(pub Vec<Arc<dyn aws_wire::service::Web>>);

impl std::fmt::Debug for Hooks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Hooks({})", self.0.len())
    }
}
