//! Who a connection is, and how it proved it.

use std::fmt;

/// The name of the console's own user (`User:__console`). It is reserved (SEC-240): no login, no stored credential and
/// no `--acl-super-user` may carry it, so the only way to be this principal is to be the console, in the process.
pub const CONSOLE_USER: &str = "__console";

/// The identity a connection is authorized as: Kafka's `KafkaPrincipal`, written `Type:name`. Only the `User`
/// type exists here.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Principal(String);

impl Principal {
    /// The user `name`, written `User:name`.
    #[must_use]
    pub fn user(name: &str) -> Self {
        Self(format!("User:{name}"))
    }

    /// The console's own principal, `User:__console`: a super user of the broker that holds it, and nobody else.
    #[must_use]
    pub fn console() -> Self {
        Self::user(CONSOLE_USER)
    }

    /// Whether `principal` (written `Type:name`) is the console's.
    #[must_use]
    pub fn is_console(principal: &str) -> bool {
        principal.strip_prefix("User:") == Some(CONSOLE_USER)
    }

    /// The principal of a connection that was never asked to log in (`User:ANONYMOUS`, as on a Kafka listener
    /// without authentication).
    #[must_use]
    pub fn anonymous() -> Self {
        Self::user("ANONYMOUS")
    }

    /// The whole principal, `User:name`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Principal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A SASL mechanism a client may log in with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mechanism {
    /// `PLAIN` (RFC 4616): the user and password in one token.
    Plain,
    /// `SCRAM-SHA-256` (RFC 7677).
    ScramSha256,
    /// `SCRAM-SHA-512`.
    ScramSha512,
}

impl Mechanism {
    /// Every mechanism there is a name for.
    pub const ALL: [Mechanism; 3] = [Self::Plain, Self::ScramSha256, Self::ScramSha512];

    /// The name on the wire and on the command line. Names are exact: the match is case-sensitive.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Plain => "PLAIN",
            Self::ScramSha256 => "SCRAM-SHA-256",
            Self::ScramSha512 => "SCRAM-SHA-512",
        }
    }

    /// The mechanism called `name` (exactly: `plain` and the empty string are not mechanisms).
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.name() == name)
    }
}

impl fmt::Display for Mechanism {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[cfg(test)]
mod tests;
