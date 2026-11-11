//! The SASL flags: `--sasl-mechanisms` and `--sasl-user`, checked in the Go edition's words.

use std::convert::Infallible;
use std::fmt;
use std::str::FromStr;
use std::sync::Arc;

use anyhow::{Result, anyhow, bail};
use kafka_auth::{Authenticator, Credentials, Mechanism};

/// A `--sasl-user` value as typed, `user:password`. Its `Debug` shows nothing: it holds a password.
#[derive(Clone)]
pub(super) struct Login(String);

impl FromStr for Login {
    type Err = Infallible;

    /// Any text: whether it is a login is judged later, in words that leave out what was typed (a parser
    /// error of clap would repeat the value, password and all).
    fn from_str(s: &str) -> Result<Self, Infallible> {
        Ok(Self(s.to_owned()))
    }
}

impl fmt::Debug for Login {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Login(<redacted>)")
    }
}

/// Who may log in: nobody is asked to unless `--sasl-mechanisms` and `--sasl-user` are both given. The checks and
/// their words are the Go edition's; none repeats what was typed after the colon.
pub(super) fn authenticator(
    mechanisms: &[String],
    users: &[Login],
) -> Result<Option<Arc<Authenticator>>> {
    if mechanisms.is_empty() && users.is_empty() {
        return Ok(None);
    }
    if mechanisms.is_empty() {
        bail!(
            "SASL users are configured but no mechanism is enabled; add --sasl-mechanisms (PLAIN, SCRAM-SHA-256 or SCRAM-SHA-512), or drop the users"
        );
    }
    let mechanisms = mechanisms
        .iter()
        .map(|name| {
            Mechanism::from_name(name).ok_or_else(|| {
                anyhow!(
                    "unknown SASL mechanism {name:?}; supported: PLAIN, SCRAM-SHA-256, SCRAM-SHA-512"
                )
            })
        })
        .collect::<Result<Vec<_>>>()?;
    if users.is_empty() {
        bail!("SASL is enabled but no credentials are configured");
    }
    let pairs = users
        .iter()
        .map(|Login(text)| {
            text.split_once(':')
                .map(|(user, password)| (user.to_owned(), password.to_owned()))
                .ok_or_else(|| anyhow!("--sasl-user: not a valid value (expected user:password)"))
        })
        .collect::<Result<Vec<_>>>()?;
    let credentials = Credentials::new(pairs)?;
    Ok(Some(Arc::new(Authenticator::new(
        &mechanisms,
        credentials,
    )?)))
}
