//! Console passwords: kept salted and hashed (PBKDF2-HMAC-SHA-256), held to the account's password policy when one is set, and
//! changed by their user with `ChangePassword`. Nothing signs in to a console locally; the password is what `ChangePassword` checks
//! and what the credential report reports on.
//!
//! Wording marked (memory) was not seen recorded.

use aws_wire::ApiError;
use hmac::{Hmac, KeyInit, Mac};
use serde_json::Value;
use sha2::Sha256;

use crate::core::Core;
use crate::held::Password;
use crate::model::Kind;
use crate::req::Req;

const ROUNDS: u32 = 4096;
/// The most earlier passwords the policy can ask about (the model: `PasswordReusePrevention` is 1 to 24).
const MAX_REMEMBERED: usize = 24;

fn hash(salt: &[u8], password: &str) -> Vec<u8> {
    let Ok(mac) = <Hmac<Sha256> as KeyInit>::new_from_slice(password.as_bytes()) else {
        return Vec::new();
    };
    let mut u = mac
        .clone()
        .chain_update(salt)
        .chain_update(1_u32.to_be_bytes())
        .finalize()
        .into_bytes();
    let mut out = u;
    for _ in 1..ROUNDS {
        u = mac.clone().chain_update(u).finalize().into_bytes();
        for (o, x) in out.iter_mut().zip(u.iter()) {
            *o ^= x;
        }
    }
    out.to_vec()
}

fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0_u8, |d, (x, y)| d | (x ^ y)) == 0
}

/// A policy flag, the characters it asks for one of, and the words for them.
type Rule = (&'static str, fn(char) -> bool, &'static str);

fn violation(message: &str) -> ApiError {
    ApiError::modeled("PasswordPolicyViolationException", message)
}

impl Core {
    /// Whether `password` meets the account's password policy (memory: AWS's sentences). With no policy set, any password does.
    fn check_policy(&self, user: &str, password: &str) -> Result<(), ApiError> {
        let Some(policy) = self
            .account()
            .password_policy
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        else {
            return Ok(());
        };
        let flag = |k: &str| policy.get(k).and_then(Value::as_bool).unwrap_or(false);
        let min = policy
            .get("MinimumPasswordLength")
            .and_then(Value::as_u64)
            .unwrap_or(6);
        if (password.chars().count() as u64) < min {
            return Err(violation(&format!(
                "Password should have a minimum length of {min}"
            )));
        }
        let rules: [Rule; 4] = [
            (
                "RequireUppercaseCharacters",
                |c| c.is_ascii_uppercase(),
                "one uppercase letter",
            ),
            (
                "RequireLowercaseCharacters",
                |c| c.is_ascii_lowercase(),
                "one lowercase letter",
            ),
            ("RequireNumbers", |c| c.is_ascii_digit(), "one number"),
            ("RequireSymbols", |c| c.is_ascii_punctuation(), "one symbol"),
        ];
        for (key, has, words) in rules {
            if flag(key) && !password.chars().any(has) {
                return Err(violation(&format!("Password should have at least {words}")));
            }
        }
        let reuse = policy
            .get("PasswordReusePrevention")
            .and_then(Value::as_u64)
            .and_then(|n| usize::try_from(n).ok())
            .unwrap_or(0);
        if let Some(p) = self.held.passwords.get(user) {
            let recent = std::iter::once((&p.salt, &p.hash))
                .chain(p.previous.iter().map(|(s, h)| (s, h)))
                .take(reuse);
            for (salt, h) in recent {
                if same(&hash(salt, password), h) {
                    return Err(violation(
                        "Password was used too recently. Please choose a different password.",
                    ));
                }
            }
        }
        Ok(())
    }

    /// Sets `user`'s password, after the policy's checks.
    pub(crate) fn set_password(
        &mut self,
        user: &str,
        password: &str,
        reset_required: bool,
    ) -> Result<(), ApiError> {
        self.check_policy(user, password)?;
        let salt = aws_seal::random_bytes::<16>()
            .ok_or_else(|| ApiError::internal("no randomness"))?
            .to_vec();
        let hashed = hash(&salt, password);
        let now = self.now_ms();
        let mut previous = self.held.passwords.remove(user).map_or_else(Vec::new, |p| {
            let mut earlier = p.previous;
            earlier.insert(0, (p.salt, p.hash));
            earlier
        });
        previous.truncate(MAX_REMEMBERED);
        self.held.passwords.insert(
            user.to_owned(),
            Password {
                salt,
                hash: hashed,
                changed_ms: now,
                reset_required,
                previous,
            },
        );
        Ok(())
    }

    /// `ChangePassword`: the caller's own, given the one it has now.
    pub(crate) fn change_password(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let arn = self.identify(r).arn;
        let user = arn
            .split_once(":user/")
            .and_then(|(_, rest)| rest.rsplit('/').next())
            .map(str::to_owned)
            .ok_or_else(|| {
                // (memory)
                ApiError::modeled(
                    "InvalidUserTypeException",
                    "The user making the call is not an IAM user.",
                )
            })?;
        let u = self.entity(Kind::User, &user)?;
        if u.password_set_ms.is_none() {
            return Err(crate::account::no_profile(&user));
        }
        // A password set before doze-aws kept them cannot be checked: the new one is taken.
        if let Some(p) = self.held.passwords.get(&user)
            && !same(&hash(&p.salt, r.s("OldPassword")), &p.hash)
        {
            // (memory) AWS's code and words for a wrong current password are not known; this is a guess, and an open question.
            return Err(ApiError::new(
                403,
                "AccessDenied",
                "The old password is incorrect.",
            ));
        }
        self.set_password(&user, r.s("NewPassword"), false)?;
        Ok(Value::Null)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 7914 section 11 gives PBKDF2-HMAC-SHA-256("passwd", "salt", 1); one round of this is that vector's first 32 bytes.
    #[test]
    fn one_round_is_the_rfcs() {
        let Ok(mac) = <Hmac<Sha256> as KeyInit>::new_from_slice(b"passwd") else {
            unreachable!()
        };
        let one = mac
            .chain_update(b"salt")
            .chain_update(1_u32.to_be_bytes())
            .finalize()
            .into_bytes();
        let hex: String = one.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(
            hex,
            "55ac046e56e3089fec1691c22544b605f94185216dde0465e68b9d57c20dacbc"
        );
    }

    #[test]
    fn a_hash_is_stable_and_tells_passwords_apart() {
        let a = hash(b"salt", "passwd");
        assert_eq!(a.len(), 32);
        assert_eq!(a, hash(b"salt", "passwd"));
        assert!(!same(&a, &hash(b"salt", "passwe")));
        assert!(!same(&a, &hash(b"tlas", "passwd")));
    }
}
