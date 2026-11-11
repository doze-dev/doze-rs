//! The users an operator configured, and a password that cannot be printed.

use std::collections::BTreeMap;
use std::fmt;

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
}

/// The configured users and their passwords. Built once at start; nothing is stored.
#[derive(Clone, Default)]
pub struct Credentials {
    users: BTreeMap<String, Secret>,
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
        let mut users = BTreeMap::new();
        for (name, password) in pairs {
            check(&name, &password)?;
            let secret = Secret(password.into_bytes().into_boxed_slice());
            if users.insert(name.clone(), secret).is_some() {
                return Err(CredentialsError::Duplicate(name));
            }
        }
        Ok(Self { users })
    }

    /// The password of the user called `name`, if there is one. The name is compared as bytes.
    #[must_use]
    pub fn password(&self, name: &[u8]) -> Option<&Secret> {
        std::str::from_utf8(name)
            .ok()
            .and_then(|name| self.users.get(name))
    }

    /// How many users there are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.users.len()
    }

    /// Whether there are no users.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.users.is_empty()
    }
}

fn check(name: &str, password: &str) -> Result<(), CredentialsError> {
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
            .field("users", &self.users.keys().collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests;
