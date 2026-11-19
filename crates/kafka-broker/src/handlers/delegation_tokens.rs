//! CreateDelegationToken (38), RenewDelegationToken (39), ExpireDelegationToken (40) and DescribeDelegationToken (41),
//! KIP-48 and KIP-373: what `kafka-delegation-tokens.sh` and the admin clients' token calls send.
//!
//! As Kafka 4.4.0 answers them (`KafkaApis.handle*TokenRequest`, `ControllerApis`, `DelegationTokenControlManager`): a
//! connection that did not log in with SASL, or logged in with a token, may not use them
//! (DELEGATION_TOKEN_REQUEST_NOT_ALLOWED, before anything else); without `delegation.token.secret.key` they answer
//! DELEGATION_TOKEN_AUTH_DISABLED. A token's id is a random Kafka `Uuid` (22 characters of base64, none a `-`), its HMAC
//! HMAC-SHA512 of the id under the key.

use bytes::Bytes;
use kafka_auth::authz::{Operation, ResourceType};
use kafka_auth::crypto::base64;
use kafka_meta::TokenRecord;
use kafka_storage::fs::Fs;
use kafka_wire::ResponseError;
use kafka_wire::StrBytes;
use kafka_wire::messages::describe_delegation_token_response::{
    DescribedDelegationToken, DescribedDelegationTokenRenewer,
};
use kafka_wire::messages::{
    CreateDelegationTokenRequest, CreateDelegationTokenResponse, DescribeDelegationTokenRequest,
    DescribeDelegationTokenResponse, ExpireDelegationTokenRequest, ExpireDelegationTokenResponse,
    RenewDelegationTokenRequest, RenewDelegationTokenResponse,
};
use tracing::warn;

use crate::tokens::Token;
use crate::{Broker, Caller, clock};

/// `DelegationTokenManager.ERROR_TIMESTAMP`: the expiry a refused renewal or expiry carries.
const ERROR_TIMESTAMP: i64 = -1;

/// The principal type every renewer must have (`KafkaPrincipal.USER_TYPE`).
const USER: &str = "User";

/// A principal as `(type, name)`.
fn split(written: &str) -> (&str, &str) {
    written.split_once(':').unwrap_or((USER, written))
}

fn text(s: &str) -> StrBytes {
    StrBytes::from_string(s.to_owned())
}

/// `now + duration`, held at `i64::MAX` (`DelegationTokenControlManager.sum`).
fn sum(now: i64, duration: i64) -> i64 {
    now.checked_add(duration).unwrap_or(i64::MAX)
}

/// A new token id: Kafka's `Uuid.randomUuid`, 16 random bytes as URL-safe base64 without padding, drawn again while it has
/// a `-` or is one of the reserved ids (zero, and the metadata topic's, `AAAAAAAAAAAAAAAAAAAAAQ`). `None` if the system has
/// no random bytes.
fn new_token_id() -> Option<String> {
    loop {
        let mut bytes = [0u8; 16];
        getrandom::fill(&mut bytes).ok()?;
        let id: String = base64::encode(&bytes)
            .trim_end_matches('=')
            .chars()
            .map(|c| match c {
                '+' => '-',
                '/' => '_',
                c => c,
            })
            .collect();
        let reserved = bytes == [0; 16] || bytes == 1u128.to_be_bytes();
        if !id.contains('-') && !reserved {
            return Some(id);
        }
    }
}

impl<F: Fs> Broker<F> {
    /// Makes a token for the caller, or for the owner it names (which needs CREATE_TOKENS on that user). Refused, the answer
    /// names the owner (and from v3 the requester) with no id, no HMAC and timestamps of -1 (`prepareResponse`); without a
    /// key, the controller's answer: AUTH_DISABLED with timestamps of 0.
    pub(crate) async fn create_delegation_token(
        &self,
        caller: &Caller,
        req: &CreateDelegationTokenRequest,
        version: i16,
    ) -> CreateDelegationTokenResponse {
        let requester = caller.principal().as_str().to_owned();
        let owner = match (&req.owner_principal_type, &req.owner_principal_name) {
            (kind, Some(name)) if !name.is_empty() => {
                format!("{}:{name}", kind.as_ref().map_or("", |k| k.as_str()))
            }
            _ => requester.clone(),
        };
        let answer = |error: Option<ResponseError>, at: i64| {
            let mut r = CreateDelegationTokenResponse::default();
            r.error_code = error.map_or(0, |e| e.code());
            let (kind, name) = split(&owner);
            r.principal_type = text(kind);
            r.principal_name = text(name);
            if version >= 3 {
                let (kind, name) = split(&requester);
                r.token_requester_principal_type = text(kind);
                r.token_requester_principal_name = text(name);
            }
            r.issue_timestamp_ms = at;
            r.expiry_timestamp_ms = at;
            r.max_timestamp_ms = at;
            r
        };
        if !caller.may_use_tokens() {
            return answer(Some(ResponseError::DelegationTokenRequestNotAllowed), -1);
        }
        let others = owner != requester
            && !self
                .authz
                .allows(caller, ResourceType::User, &owner, Operation::CreateTokens);
        if others {
            return answer(Some(ResponseError::DelegationTokenAuthorizationFailed), -1);
        }
        if req
            .renewers
            .iter()
            .any(|r| r.principal_type.as_str() != USER)
        {
            return answer(Some(ResponseError::InvalidPrincipalType), -1);
        }
        let config = &self.config.delegation_tokens;
        if config.secret.is_none() {
            return answer(Some(ResponseError::DelegationTokenAuthDisabled), 0);
        }
        let _one_change_at_a_time = self.token_admin.lock().await;
        let now = clock::now().get();
        let lifetime = if req.max_lifetime_ms > 0 {
            config.max_lifetime_ms.min(req.max_lifetime_ms)
        } else {
            config.max_lifetime_ms
        };
        let max = sum(now, lifetime);
        let expiry = max.min(sum(now, config.expiry_time_ms));
        let Some(id) = new_token_id() else {
            return answer(Some(ResponseError::UnknownServerError), 0);
        };
        let Some(mac) = self.token_hmac(&id) else {
            return answer(Some(ResponseError::DelegationTokenAuthDisabled), 0);
        };
        let record = TokenRecord {
            owner: owner.clone(),
            requester: requester.clone(),
            renewers: (req.renewers.iter())
                .map(|r| {
                    format!(
                        "{}:{}",
                        r.principal_type.as_str(),
                        r.principal_name.as_str()
                    )
                })
                .collect(),
            issue_ms: now,
            max_ms: max,
            expiry_ms: expiry,
        };
        if let Err(e) = self.keep_token(Token {
            id: id.clone(),
            record,
            hmac: mac,
        }) {
            warn!(error = %e, "a delegation token could not be kept");
            return answer(Some(ResponseError::KafkaStorageError), 0);
        }
        let mut r = answer(None, now);
        r.expiry_timestamp_ms = expiry;
        r.max_timestamp_ms = max;
        r.token_id = text(&id);
        r.hmac = Bytes::copy_from_slice(&mac);
        r
    }

    /// Moves a token's expiry to now plus the period asked for (the configured one if none, and never past its maximum).
    pub(crate) async fn renew_delegation_token(
        &self,
        caller: &Caller,
        req: &RenewDelegationTokenRequest,
    ) -> RenewDelegationTokenResponse {
        let mut r = RenewDelegationTokenResponse::default();
        if !caller.may_use_tokens() {
            r.error_code = ResponseError::DelegationTokenRequestNotAllowed.code();
            r.expiry_timestamp_ms = ERROR_TIMESTAMP;
            return r;
        }
        let config = &self.config.delegation_tokens;
        if config.secret.is_none() {
            r.error_code = ResponseError::DelegationTokenAuthDisabled.code();
            return r;
        }
        let _one_change_at_a_time = self.token_admin.lock().await;
        let now = clock::now().get();
        let Some(mut token) = self.tokens.by_hmac(&req.hmac) else {
            r.error_code = ResponseError::DelegationTokenNotFound.code();
            return r;
        };
        if token.expired(now) {
            r.error_code = ResponseError::DelegationTokenExpired.code();
            return r;
        }
        if !token.may_renew(caller.principal().as_str()) {
            r.error_code = ResponseError::DelegationTokenOwnerMismatch.code();
            return r;
        }
        let period = if req.renew_period_ms > 0 {
            config.expiry_time_ms.min(req.renew_period_ms)
        } else {
            config.expiry_time_ms
        };
        let expiry = token.record.max_ms.min(sum(now, period));
        token.record.expiry_ms = expiry;
        if let Err(e) = self.keep_token(token) {
            warn!(error = %e, "a delegation token's renewal could not be kept");
            r.error_code = ResponseError::KafkaStorageError.code();
            return r;
        }
        r.expiry_timestamp_ms = expiry;
        r
    }

    /// Moves a token's expiry to now plus the period asked for (never past its maximum), or, for a negative period, removes
    /// it at once.
    pub(crate) async fn expire_delegation_token(
        &self,
        caller: &Caller,
        req: &ExpireDelegationTokenRequest,
    ) -> ExpireDelegationTokenResponse {
        let mut r = ExpireDelegationTokenResponse::default();
        if !caller.may_use_tokens() {
            r.error_code = ResponseError::DelegationTokenRequestNotAllowed.code();
            r.expiry_timestamp_ms = ERROR_TIMESTAMP;
            return r;
        }
        if self.config.delegation_tokens.secret.is_none() {
            r.error_code = ResponseError::DelegationTokenAuthDisabled.code();
            return r;
        }
        let _one_change_at_a_time = self.token_admin.lock().await;
        let now = clock::now().get();
        let Some(mut token) = self.tokens.by_hmac(&req.hmac) else {
            r.error_code = ResponseError::DelegationTokenNotFound.code();
            return r;
        };
        if !token.may_renew(caller.principal().as_str()) {
            r.error_code = ResponseError::DelegationTokenOwnerMismatch.code();
            return r;
        }
        let kept = if req.expiry_time_period_ms < 0 {
            r.expiry_timestamp_ms = now;
            self.drop_tokens(&[token.id.as_str()])
        } else if token.expired(now) {
            r.error_code = ResponseError::DelegationTokenExpired.code();
            return r;
        } else {
            let expiry = token.record.max_ms.min(sum(now, req.expiry_time_period_ms));
            r.expiry_timestamp_ms = expiry;
            token.record.expiry_ms = expiry;
            self.keep_token(token)
        };
        if let Err(e) = kept {
            warn!(error = %e, "a delegation token's expiry could not be kept");
            return ExpireDelegationTokenResponse::default()
                .with_error_code(ResponseError::KafkaStorageError.code());
        }
        r
    }

    /// The tokens the caller may see, of the owners named (all, for a null list; none, for an empty one): its own, those it
    /// asked for or may renew, and those it has DESCRIBE on (the token) or DESCRIBE_TOKENS on (the owner) for
    /// (`DelegationTokenManager.filterToken`). Expired tokens are listed until the sweep removes them.
    pub(crate) fn describe_delegation_token(
        &self,
        caller: &Caller,
        req: &DescribeDelegationTokenRequest,
        version: i16,
    ) -> DescribeDelegationTokenResponse {
        let mut r = DescribeDelegationTokenResponse::default();
        if !caller.may_use_tokens() {
            r.error_code = ResponseError::DelegationTokenRequestNotAllowed.code();
            return r;
        }
        if self.config.delegation_tokens.secret.is_none() {
            r.error_code = ResponseError::DelegationTokenAuthDisabled.code();
            return r;
        }
        let owners: Option<Vec<String>> = req.owners.as_ref().map(|owners| {
            (owners.iter())
                .map(|o| {
                    format!(
                        "{}:{}",
                        o.principal_type.as_str(),
                        o.principal_name.as_str()
                    )
                })
                .collect()
        });
        if owners.as_ref().is_some_and(Vec::is_empty) {
            return r;
        }
        let me = caller.principal().as_str();
        r.tokens = (self.tokens.all().into_iter())
            .filter(|t| {
                if let Some(owners) = &owners {
                    if !owners.iter().any(|o| t.owner_or_renewer(o)) {
                        return false;
                    }
                }
                t.owner_or_renewer(me)
                    || self.authz.allows(
                        caller,
                        ResourceType::DelegationToken,
                        &t.id,
                        Operation::Describe,
                    )
                    || self.authz.allows(
                        caller,
                        ResourceType::User,
                        &t.record.owner,
                        Operation::DescribeTokens,
                    )
            })
            .map(|t| described(&t, version))
            .collect();
        r
    }
}

fn described(t: &Token, version: i16) -> DescribedDelegationToken {
    let mut d = DescribedDelegationToken::default();
    let (kind, name) = split(&t.record.owner);
    d.principal_type = text(kind);
    d.principal_name = text(name);
    if version >= 3 {
        let (kind, name) = split(&t.record.requester);
        d.token_requester_principal_type = text(kind);
        d.token_requester_principal_name = text(name);
    }
    d.issue_timestamp = t.record.issue_ms;
    d.expiry_timestamp = t.record.expiry_ms;
    d.max_timestamp = t.record.max_ms;
    d.token_id = text(&t.id);
    d.hmac = Bytes::copy_from_slice(&t.hmac);
    d.renewers = (t.record.renewers.iter())
        .map(|w| {
            let (kind, name) = split(w);
            let mut r = DescribedDelegationTokenRenewer::default();
            r.principal_type = text(kind);
            r.principal_name = text(name);
            r
        })
        .collect();
    d
}
