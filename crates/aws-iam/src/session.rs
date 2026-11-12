//! A session is a signed statement, not a stored record: `base64(payload) "." base64(HMAC-SHA-256(account key, payload))`. STS hands
//! it out as the session token; the access key id and secret that go with it are derived from the payload and the account key. So
//! there is nothing to keep, nothing lost at a restart, and nothing to expire: a token is good until the time written in it.
//! (Revocation is not supported, and is not pretended.)

use aws_core::base64;
use hmac::{Hmac, KeyInit, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

/// What kind of session this is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum SessionKind {
    /// `AssumeRole` and its web-identity and SAML forms.
    Role,
    /// `GetSessionToken`: the user's own, with a time limit.
    User,
    /// `GetFederationToken`.
    Federated,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Session {
    pub(crate) kind: SessionKind,
    pub(crate) account: String,
    /// The role's ARN, or the user's name, or the federated name.
    pub(crate) subject: String,
    /// The role session name.
    pub(crate) name: String,
    /// The principal id the session shows (`AROA...:name`).
    pub(crate) id: String,
    pub(crate) expires_ms: i64,
    /// A session policy, carried and not yet evaluated.
    pub(crate) policy: Option<String>,
    pub(crate) tags: Vec<(String, String)>,
    pub(crate) source_identity: Option<String>,
    /// Distinguishes two sessions made in the same millisecond.
    pub(crate) nonce: String,
}

fn mac(key: &[u8], data: &[u8]) -> Vec<u8> {
    let Ok(mut m) = <Hmac<Sha256> as KeyInit>::new_from_slice(key) else {
        return Vec::new();
    };
    m.update(data);
    m.finalize().into_bytes().to_vec()
}

fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0_u8, |d, (x, y)| d | (x ^ y)) == 0
}

/// 16 characters of IAM's alphabet derived from `bytes`.
fn letters(bytes: &[u8]) -> String {
    const SET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    bytes
        .iter()
        .take(16)
        .map(|b| {
            char::from(
                SET.get(usize::from(*b) % SET.len())
                    .copied()
                    .unwrap_or(b'A'),
            )
        })
        .collect()
}

impl Session {
    fn payload(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }

    /// The session token.
    pub(crate) fn token(&self, key: &[u8]) -> String {
        let payload = self.payload();
        format!(
            "{}.{}",
            base64::encode(&payload),
            base64::encode(&mac(key, &payload))
        )
    }

    /// The access key id that goes with the token: `ASIA` and sixteen letters.
    pub(crate) fn access_key_id(&self, key: &[u8]) -> String {
        format!(
            "ASIA{}",
            letters(&mac(key, &[b"id:".as_slice(), &self.payload()].concat()))
        )
    }

    /// The secret access key that goes with the token.
    pub(crate) fn secret_access_key(&self, key: &[u8]) -> String {
        base64::encode(&mac(
            key,
            &[b"secret:".as_slice(), &self.payload()].concat(),
        ))
        .chars()
        .take(40)
        .collect()
    }

    /// The session a token stands for if it is ours, whether or not it has run out (a caller of an expired one is told so).
    pub(crate) fn read(token: &str, key: &[u8]) -> Option<Self> {
        let (payload, tag) = token.split_once('.')?;
        let payload = base64::decode(payload)?;
        let tag = base64::decode(tag)?;
        if !same(&mac(key, &payload), &tag) {
            return None;
        }
        serde_json::from_slice(&payload).ok()
    }

    /// The session a token stands for, if it is ours and has not run out.
    pub(crate) fn verify(token: &str, key: &[u8], now_ms: i64) -> Result<Self, TokenFault> {
        let (payload, tag) = token.split_once('.').ok_or(TokenFault::Invalid)?;
        let payload = base64::decode(payload).ok_or(TokenFault::Invalid)?;
        let tag = base64::decode(tag).ok_or(TokenFault::Invalid)?;
        if !same(&mac(key, &payload), &tag) {
            return Err(TokenFault::Invalid);
        }
        let session: Self = serde_json::from_slice(&payload).map_err(|_| TokenFault::Invalid)?;
        if session.expires_ms <= now_ms {
            return Err(TokenFault::Expired);
        }
        Ok(session)
    }
}

/// Why a token was not accepted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TokenFault {
    Invalid,
    Expired,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn session() -> Session {
        Session {
            kind: SessionKind::Role,
            account: "000000000000".into(),
            subject: "arn:aws:iam::000000000000:role/r".into(),
            name: "s".into(),
            id: "AROAX:s".into(),
            expires_ms: 10_000,
            policy: None,
            tags: vec![("k".into(), "v".into())],
            source_identity: None,
            nonce: "n".into(),
        }
    }

    #[test]
    fn a_token_verifies_until_its_time_and_not_with_another_key() {
        let key = [3_u8; 32];
        let token = session().token(&key);
        assert_eq!(Session::verify(&token, &key, 5_000).unwrap().name, "s");
        assert_eq!(
            Session::verify(&token, &key, 10_000).unwrap_err(),
            TokenFault::Expired
        );
        assert_eq!(
            Session::verify(&token, &[4_u8; 32], 5_000).unwrap_err(),
            TokenFault::Invalid
        );
        assert_eq!(
            Session::verify("junk", &key, 0).unwrap_err(),
            TokenFault::Invalid
        );
        let (payload, tag) = token.split_once('.').unwrap();
        let forged = format!(
            "{}.{tag}",
            base64::encode(&[payload.as_bytes(), b"x"].concat())
        );
        assert_eq!(
            Session::verify(&forged, &key, 0).unwrap_err(),
            TokenFault::Invalid
        );
    }

    #[test]
    fn the_key_id_and_secret_follow_the_session() {
        let key = [3_u8; 32];
        let mut other = session();
        other.nonce = "other".into();
        assert!(
            session().access_key_id(&key).starts_with("ASIA")
                && session().access_key_id(&key).len() == 20
        );
        assert_eq!(session().access_key_id(&key), session().access_key_id(&key));
        assert_ne!(session().access_key_id(&key), other.access_key_id(&key));
        assert_eq!(session().secret_access_key(&key).len(), 40);
    }
}
