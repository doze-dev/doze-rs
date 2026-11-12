//! Where the secrets are. The verifier asks for a credential by access key id and gets to look at it for the length of a
//! closure, so a secret never leaves its owner by value.

use std::collections::HashMap;
use std::fmt;

/// A secret access key. Never printed.
pub struct Secret(String);

impl Secret {
    /// Wraps a secret.
    #[must_use]
    pub const fn new(s: String) -> Self {
        Self(s)
    }

    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

/// One stored credential, as far as signature checking is concerned. Who the credential *is* (its principal) is the
/// credential store's business, not this crate's.
#[derive(Debug)]
pub struct Credential {
    /// The secret access key.
    pub secret: Secret,
    /// Present for temporary credentials; the request must carry the same token.
    pub session_token: Option<String>,
    /// Unix seconds; `None` for long-lived keys.
    pub expires_at: Option<i64>,
}

/// A source of credentials.
pub trait Credentials {
    /// Runs `f` on the credential for `access_key_id`, or on `None` if there is none.
    fn lookup<R>(&self, access_key_id: &str, f: impl FnOnce(Option<&Credential>) -> R) -> R;
}

/// A fixed set of credentials: for tests, and for the one-user case.
#[derive(Debug, Default)]
pub struct StaticCredentials {
    map: HashMap<String, Credential>,
}

impl StaticCredentials {
    /// An empty set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds or replaces a credential.
    pub fn insert(&mut self, access_key_id: impl Into<String>, credential: Credential) {
        self.map.insert(access_key_id.into(), credential);
    }
}

impl Credentials for StaticCredentials {
    fn lookup<R>(&self, access_key_id: &str, f: impl FnOnce(Option<&Credential>) -> R) -> R {
        f(self.map.get(access_key_id))
    }
}

/// A source of credentials that has to be asked, because it keeps them somewhere that answers later (IAM's access keys and role
/// sessions live behind an actor). The gateway asks it, once per request, for a key id that its fixed credentials do not hold.
pub trait CredentialSource: Send + Sync + fmt::Debug + 'static {
    /// The credential for `access_key_id`, if this source issued it. `session_token` is the token the request carried.
    fn find<'a>(
        &'a self,
        access_key_id: &'a str,
        session_token: Option<&'a str>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<Credential>> + Send + 'a>>;
}

#[derive(Debug)]
/// Fixed credentials with, beside them, at most one the request's source supplied.
pub struct WithExtra<'a> {
    /// The fixed set.
    pub fixed: &'a StaticCredentials,
    /// The key id and credential a [`CredentialSource`] supplied for this request.
    pub extra: Option<(&'a str, &'a Credential)>,
}

impl Credentials for WithExtra<'_> {
    fn lookup<R>(&self, access_key_id: &str, f: impl FnOnce(Option<&Credential>) -> R) -> R {
        match self.extra {
            Some((id, credential)) if id == access_key_id => f(Some(credential)),
            _ => self.fixed.lookup(access_key_id, f),
        }
    }
}
