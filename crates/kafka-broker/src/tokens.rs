//! Delegation tokens (KIP-48): the settings, the tokens in force, and how they are kept.
//!
//! Kafka 4.4.0 keeps a token as a `DelegationTokenRecord` in its metadata log and its HMAC nowhere: the HMAC is
//! HMAC-SHA512 of the token's id under `delegation.token.secret.key`, made again whenever it is needed. Here the record is a
//! row of the metadata database (`kafka_meta::TokenRecord`), the tokens in force are held in memory, and the SCRAM
//! verifiers a login with a token checks are in [`kafka_auth::Users`], which the server's authenticator shares. A change is
//! written to the table first, then put in force, so a login never sees a token the table does not have.
//!
//! Without a secret key nothing here is in use (`DelegationTokenManager.isEnabled`): every token API answers
//! DELEGATION_TOKEN_AUTH_DISABLED, and the tokens a run with a key made stay in the table, unused, until a run with a key
//! loads them.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, PoisonError, RwLock};
use std::time::Duration;

use kafka_auth::tokens::{TokenLogin, TokenLogins, hmac};
use kafka_auth::{Principal, Secret, Users};
use kafka_meta::{MetaStore, TokenRecord};
use kafka_storage::fs::Fs;
use tracing::{info, warn};

use crate::{Broker, BrokerError, blocking, clock};

/// Kafka's `delegation.token.*` settings.
#[derive(Clone)]
pub struct TokenConfig {
    /// `delegation.token.secret.key`: tokens are on only when it is set.
    pub secret: Option<Arc<Secret>>,
    /// `delegation.token.max.lifetime.ms`: the longest a token can live, renewals and all (7 days).
    pub max_lifetime_ms: i64,
    /// `delegation.token.expiry.time.ms`: how long a token lives from its making or its last renewal unless asked for less
    /// (1 day).
    pub expiry_time_ms: i64,
    /// `delegation.token.expiry.check.interval.ms`: how often the expired tokens are removed (1 hour). Until then an expired
    /// token is still described, and still logs in, as in Kafka.
    pub expiry_check_every: Duration,
}

impl Default for TokenConfig {
    fn default() -> Self {
        Self {
            secret: None,
            max_lifetime_ms: 7 * 24 * 3600 * 1000,
            expiry_time_ms: 24 * 3600 * 1000,
            expiry_check_every: Duration::from_secs(3600),
        }
    }
}

impl fmt::Debug for TokenConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TokenConfig")
            .field("enabled", &self.secret.is_some())
            .field("max_lifetime_ms", &self.max_lifetime_ms)
            .field("expiry_time_ms", &self.expiry_time_ms)
            .field("expiry_check_every", &self.expiry_check_every)
            .finish()
    }
}

/// One token: its record, and its HMAC (what the holder proves it has).
#[derive(Clone)]
pub(crate) struct Token {
    pub(crate) id: String,
    pub(crate) record: TokenRecord,
    pub(crate) hmac: [u8; 64],
}

impl Token {
    /// Whether `who` is its owner, the one who asked for it, or a renewer (`TokenInformation.ownerOrRenewer`).
    pub(crate) fn owner_or_renewer(&self, who: &str) -> bool {
        self.record.owner == who || self.record.requester == who || self.renewer(who)
    }

    /// Whether `who` may renew or expire it: its owner or a renewer, not the one who asked for it
    /// (`DelegationTokenControlManager.allowedToRenew`).
    pub(crate) fn may_renew(&self, who: &str) -> bool {
        self.record.owner == who || self.renewer(who)
    }

    fn renewer(&self, who: &str) -> bool {
        self.record.renewers.iter().any(|r| r == who)
    }

    /// Whether it is past its expiry or its maximum lifetime at `now_ms`.
    pub(crate) fn expired(&self, now_ms: i64) -> bool {
        self.record.max_ms < now_ms || self.record.expiry_ms < now_ms
    }
}

/// The tokens in force, by id; empty while tokens are off.
#[derive(Default)]
pub(crate) struct Tokens(RwLock<BTreeMap<String, Token>>);

impl Tokens {
    /// Every token, by id.
    pub(crate) fn all(&self) -> Vec<Token> {
        self.0
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .cloned()
            .collect()
    }

    /// The token whose HMAC is `mac` (`DelegationTokenCache.tokenForHmac`).
    pub(crate) fn by_hmac(&self, mac: &[u8]) -> Option<Token> {
        let all = self.0.read().unwrap_or_else(PoisonError::into_inner);
        all.values()
            .find(|t| kafka_auth::crypto::ct_eq(&t.hmac, mac))
            .cloned()
    }

    fn put(&self, token: Token) {
        let mut all = self.0.write().unwrap_or_else(PoisonError::into_inner);
        all.insert(token.id.clone(), token);
    }

    fn remove(&self, ids: &[&str]) {
        let mut all = self.0.write().unwrap_or_else(PoisonError::into_inner);
        for id in ids {
            all.remove(*id);
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.0.read().unwrap_or_else(PoisonError::into_inner).len()
    }
}

impl fmt::Debug for Tokens {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Tokens({})", self.len())
    }
}

/// A principal written `Type:name`, as a token keeps it, made one a login can be: Kafka's `KafkaPrincipal`.
fn principal_of(written: &str) -> Principal {
    match written.split_once(':') {
        Some((kind, name)) => Principal::of_type(kind, name),
        None => Principal::user(written),
    }
}

/// Puts the stored tokens in force, when tokens are on: each one's HMAC under the key, and its SCRAM verifiers.
pub(crate) fn load(
    meta: &MetaStore,
    config: &TokenConfig,
    users: &Users,
) -> Result<Tokens, BrokerError> {
    let tokens = Tokens::default();
    let Some(secret) = &config.secret else {
        return Ok(tokens);
    };
    let mut logins = users.tokens().as_ref().clone();
    for (id, record) in meta.tokens()? {
        let mac = hmac(secret.expose(), &id);
        match TokenLogin::derive(principal_of(&record.owner), &mac) {
            Ok(login) => logins = logins.with(&id, login),
            Err(e) => {
                warn!(error = %e, "a delegation token cannot log in: no random bytes for its salt")
            }
        }
        tokens.put(Token {
            id,
            record,
            hmac: mac,
        });
    }
    users.replace_tokens(logins);
    Ok(tokens)
}

impl<F: Fs> Broker<F> {
    /// The HMAC of the token `id` under the secret key; `None` while tokens are off.
    pub(crate) fn token_hmac(&self, id: &str) -> Option<[u8; 64]> {
        let secret = self.config.delegation_tokens.secret.as_ref()?;
        Some(hmac(secret.expose(), id))
    }

    /// Keeps `token` (new, or its record changed), then puts it in force. Whoever calls holds `token_admin`.
    ///
    /// # Errors
    /// The write failed; nothing is changed.
    pub(crate) fn keep_token(&self, token: Token) -> Result<(), BrokerError> {
        blocking::run(|| self.meta.alter_tokens(&[(&token.id, &token.record)], &[]))?;
        let logins = self.users.tokens();
        let login = TokenLogin::derive(principal_of(&token.record.owner), &token.hmac);
        match login {
            Ok(login) => self.users.replace_tokens(logins.with(&token.id, login)),
            Err(e) => {
                warn!(error = %e, "a delegation token cannot log in: no random bytes for its salt")
            }
        }
        self.tokens.put(token);
        Ok(())
    }

    /// Removes the tokens `ids`: from the table, then from what logins and requests see. Whoever calls holds `token_admin`.
    ///
    /// # Errors
    /// The write failed; nothing is changed.
    pub(crate) fn drop_tokens(&self, ids: &[&str]) -> Result<(), BrokerError> {
        if ids.is_empty() {
            return Ok(());
        }
        blocking::run(|| self.meta.alter_tokens(&[], ids))?;
        let logins: TokenLogins = self.users.tokens().without(ids.iter().copied());
        self.users.replace_tokens(logins);
        self.tokens.remove(ids);
        Ok(())
    }

    /// Removes every token past its expiry or its maximum lifetime (`DelegationTokenControlManager.sweepExpiredDelegationTokens`).
    pub async fn sweep_expired_tokens(&self) {
        let _one_change_at_a_time = self.token_admin.lock().await;
        let now = clock::now().get();
        let all = self.tokens.all();
        let expired: Vec<&str> = (all.iter())
            .filter(|t| t.expired(now))
            .map(|t| t.id.as_str())
            .collect();
        if expired.is_empty() {
            return;
        }
        match self.drop_tokens(&expired) {
            Ok(()) => info!(tokens = expired.len(), "expired delegation tokens removed"),
            Err(e) => {
                warn!(error = %e, "removing expired delegation tokens failed; the next sweep tries again")
            }
        }
    }

    /// Removes the expired tokens every `delegation.token.expiry.check.interval.ms`, as long as the broker lives; nothing
    /// is started while tokens are off.
    pub fn start_token_sweeper(self: &Arc<Self>) -> Option<tokio::task::JoinHandle<()>>
    where
        F: Send + Sync + 'static,
    {
        self.config.delegation_tokens.secret.as_ref()?;
        let broker = Arc::downgrade(self);
        let every = self.config.delegation_tokens.expiry_check_every;
        Some(tokio::spawn(async move {
            loop {
                tokio::time::sleep(every).await;
                let Some(broker) = broker.upgrade() else {
                    break;
                };
                broker.sweep_expired_tokens().await;
            }
        }))
    }
}
