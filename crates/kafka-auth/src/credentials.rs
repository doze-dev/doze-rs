//! The users an operator configured, and a password that cannot be printed.
//!
//! PLAIN needs the password itself. SCRAM needs only a verifier, derived once at start for each user and each
//! SCRAM mechanism that is on, with a salt of its own; nothing is stored, so the verifiers are made again, with
//! new salts, at every start.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::sync::Arc;

use crate::scram::keys::ITERATIONS;
use crate::scram::{Kind, Verifier};

/// A password. It has no `Display`, and its `Debug` shows nothing of it, so it cannot reach a log line by
/// accident; the only way to see the bytes is [`Secret::expose`], and each use of that is a decision.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(Box<[u8]>);

impl Secret {
    /// The password's bytes.
    #[must_use]
    pub fn expose(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

/// A user and password pair that cannot be a login. The text names the user, never the password.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum CredentialsError {
    /// A user with no name.
    #[error("SASL credential has an empty username")]
    EmptyUser,
    /// A user name that cannot be sent in a PLAIN token (it holds a NUL).
    #[error("SASL user {0:?} has a NUL byte in its name")]
    NulInUser(String),
    /// A user with no password.
    #[error("SASL user {0:?} has an empty password")]
    EmptyPassword(String),
    /// A password outside ASCII, or with a NUL: passwords are not normalized (no SASLprep), so only the bytes
    /// every client sends the same way are allowed.
    #[error("SASL user {0:?} has a password that is not ASCII text without NUL bytes")]
    UnusablePassword(String),
    /// The same user twice.
    #[error("SASL user {0:?} is given more than once")]
    Duplicate(String),
    /// The system could not give the random bytes a salt needs.
    #[error("the system gave no random bytes for a SCRAM salt")]
    NoEntropy,
}

/// The configured users and their passwords. Built once at start; nothing is stored.
#[derive(Clone, Default)]
pub struct Credentials {
    /// Every user, whether or not their password is still held.
    names: BTreeSet<String>,
    /// The passwords still held: all of them, until [`Credentials::forget_passwords`].
    secrets: BTreeMap<String, Secret>,
    scram: HashMap<Kind, BTreeMap<String, Arc<Verifier>>>,
}

impl Credentials {
    /// The users in `pairs` (`(name, password)`).
    ///
    /// # Errors
    /// An empty name or password, a NUL in a name, a password outside ASCII, or a name given twice.
    pub fn new<I>(pairs: I) -> Result<Self, CredentialsError>
    where
        I: IntoIterator<Item = (String, String)>,
    {
        let mut secrets = BTreeMap::new();
        for (name, password) in pairs {
            validate(&name, &password)?;
            let secret = Secret(password.into_bytes().into_boxed_slice());
            if secrets.insert(name.clone(), secret).is_some() {
                return Err(CredentialsError::Duplicate(name));
            }
        }
        Ok(Self {
            names: secrets.keys().cloned().collect(),
            secrets,
            scram: HashMap::new(),
        })
    }

    /// These credentials without the passwords: the names and the SCRAM verifiers stay, and [`Credentials::password`]
    /// finds nothing. For a broker that offers no PLAIN, which is the only use a password has once the verifiers are
    /// made (SEC-D59). Call it after [`Credentials::with_scram`]: a verifier cannot be made from a forgotten password.
    #[must_use]
    pub fn forget_passwords(mut self) -> Self {
        self.secrets.clear();
        self
    }

    /// These credentials with a SCRAM verifier for every user under each of `kinds`: `ITERATIONS` rounds and a
    /// salt of 16 random bytes, new for each.
    ///
    /// # Errors
    /// [`CredentialsError::NoEntropy`] if the system has no random bytes to give.
    pub fn with_scram(mut self, kinds: &[Kind]) -> Result<Self, CredentialsError> {
        for &kind in kinds {
            let mut verifiers = BTreeMap::new();
            for (name, password) in &self.secrets {
                let mut salt = [0u8; 16];
                getrandom::fill(&mut salt).map_err(|_| CredentialsError::NoEntropy)?;
                let verifier = Verifier::derive(kind, password.expose(), &salt, ITERATIONS);
                verifiers.insert(name.clone(), Arc::new(verifier));
            }
            self.scram.insert(kind, verifiers);
        }
        Ok(self)
    }

    /// The SCRAM verifier of the user called `name` for `kind`, if there is one.
    #[must_use]
    pub fn verifier(&self, kind: Kind, name: &str) -> Option<Arc<Verifier>> {
        self.scram.get(&kind)?.get(name).cloned()
    }

    /// Whether there is a user called `name`.
    #[must_use]
    pub fn has_user(&self, name: &str) -> bool {
        self.names.contains(name)
    }

    /// The names of the users.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.names.iter().map(String::as_str)
    }

    /// The mechanisms `name` has a SCRAM verifier for, with the iteration count of each, in mechanism order.
    #[must_use]
    pub fn scram_of(&self, name: &str) -> Vec<(Kind, u32)> {
        Kind::ALL
            .iter()
            .filter_map(|&k| Some((k, self.verifier(k, name)?.iterations().get())))
            .collect()
    }

    /// The password of the user called `name`, if there is one. The name is compared as bytes.
    #[must_use]
    pub fn password(&self, name: &[u8]) -> Option<&Secret> {
        std::str::from_utf8(name)
            .ok()
            .and_then(|name| self.secrets.get(name))
    }

    /// How many users there are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.names.len()
    }

    /// Whether there are no users.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }
}

/// Whether `name` and `password` can be a login: a name and a password that every client sends the same way.
///
/// # Errors
/// The first thing wrong with them, in words that name the user and never the password.
pub fn validate(name: &str, password: &str) -> Result<(), CredentialsError> {
    if name.is_empty() {
        return Err(CredentialsError::EmptyUser);
    }
    if name.contains('\0') {
        return Err(CredentialsError::NulInUser(name.to_owned()));
    }
    if password.is_empty() {
        return Err(CredentialsError::EmptyPassword(name.to_owned()));
    }
    if !password.is_ascii() || password.contains('\0') {
        return Err(CredentialsError::UnusablePassword(name.to_owned()));
    }
    Ok(())
}

impl fmt::Debug for Credentials {
    /// The names of the users, and nothing of their passwords.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credentials")
            .field("users", &self.names)
            .field("scram", &self.scram.keys().collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests;
