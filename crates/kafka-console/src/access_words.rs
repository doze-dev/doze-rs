//! The words of the Access page: the state it is in, the three start-up warnings and whether each holds now, and the
//! sentences under the switches. Pure functions of what the page already read; nothing here asks the broker anything.

use crate::model::{AccessWarning, Span};

/// What is on, as far as the title word goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Posture {
    /// No sign-in, the default allows, no ACL.
    Open,
    /// Nobody signs in but ACLs (or a default that denies) decide for `User:ANONYMOUS`.
    AclsOnly,
    /// Clients sign in.
    SignIn,
}

/// The posture of a broker from its three switches.
#[must_use]
pub const fn posture(sasl: bool, acls: usize, default_allow: bool) -> Posture {
    if sasl {
        Posture::SignIn
    } else if acls > 0 || !default_allow {
        Posture::AclsOnly
    } else {
        Posture::Open
    }
}

impl Posture {
    /// The word beside the title.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::Open => "Open",
            Self::AclsOnly => "ACLs only",
            Self::SignIn => "Sign-in required",
        }
    }
}

/// What the warnings are decided from.
#[derive(Debug, Clone, Copy)]
pub struct Switches<'a> {
    pub sasl: bool,
    pub tls: bool,
    pub default_allow: bool,
    /// The Kafka listener can be reached from other machines.
    pub listener_beyond_loopback: bool,
    /// The console can be reached from other machines.
    pub console_beyond_loopback: bool,
    pub console_addr: &'a str,
    pub listener: &'a str,
}

fn plain(text: &str) -> Vec<Span> {
    vec![Span::text(text)]
}

/// The three warnings the broker says at start (SEC-188, SEC-189, SEC-191), each with whether its condition holds.
#[must_use]
pub fn warnings(s: &Switches<'_>) -> Vec<AccessWarning> {
    let protected = s.sasl || s.tls;
    let one = AccessWarning {
        title: plain("Console served beyond loopback with SASL or TLS on"),
        because: if s.console_beyond_loopback && protected {
            vec![
                Span::text("The console is on "),
                Span::mono(s.console_addr),
                Span::text(": it has no login and is plain HTTP, and this was accepted with "),
                Span::mono("--console-unprotected"),
            ]
        } else if s.console_beyond_loopback {
            vec![
                Span::text("The console is on "),
                Span::mono(s.console_addr),
                Span::text(", and neither SASL nor TLS is on"),
            ]
        } else {
            vec![Span::text("The console is on "), Span::mono(s.console_addr)]
        },
        holds: s.console_beyond_loopback && protected,
    };
    let two = AccessWarning {
        title: plain("SASL without TLS beyond loopback"),
        because: if !s.sasl {
            plain("SASL is off")
        } else if s.tls {
            plain("TLS is on, so a password does not cross the network as it is")
        } else if s.listener_beyond_loopback {
            plain("A PLAIN password would cross the network as it is")
        } else {
            plain("The listener is loopback, so nothing crosses a network")
        },
        holds: s.sasl && !s.tls && s.listener_beyond_loopback,
    };
    let three = AccessWarning {
        title: vec![
            Span::text("SASL on with "),
            Span::mono("--acl-default allow"),
        ],
        because: if !s.sasl {
            plain("SASL is off")
        } else if s.default_allow {
            vec![
                Span::text("Every logged-in user could manage users and ACLs: use "),
                Span::mono("deny"),
                Span::text(" or "),
                Span::mono("--acl-super-user"),
            ]
        } else {
            plain("The default is deny")
        },
        holds: s.sasl && s.default_allow,
    };
    vec![one, two, three]
}

/// `1 from flags, 3 stored`.
#[must_use]
pub fn users_line(flags: usize, stored: usize) -> String {
    format!("{flags} from flags, {stored} stored")
}

/// `7 allow, 1 deny`.
#[must_use]
pub fn acl_split(allow: usize, deny: usize) -> String {
    format!("{allow} allow, {deny} deny")
}

/// The sentence under the title.
#[must_use]
pub fn sub(p: Posture, listen: &str, mechanisms: &[String], tls: bool) -> Vec<Span> {
    match p {
        Posture::Open => vec![
            Span::text("Anyone who can reach "),
            Span::strong(listen),
            Span::text(
                " may read, write and change anything: nobody signs in, no ACL exists and the default allows. This page shows each of those three switches and what it holds.",
            ),
        ],
        Posture::AclsOnly => vec![
            Span::text("Nobody signs in to "),
            Span::strong(listen),
            Span::text(", so every client is "),
            Span::mono("User:ANONYMOUS"),
            Span::text(
                " and the ACLs decide what it may do. Everything below is read from the broker.",
            ),
        ],
        Posture::SignIn => vec![Span::text(format!(
            "Clients sign in with {}{}, and ACLs decide what each one may do. Everything below is read from the broker; no password, key or salt is ever shown.",
            mechanisms.join(" or "),
            if tls { " over TLS" } else { "" }
        ))],
    }
}

#[cfg(test)]
mod tests;
