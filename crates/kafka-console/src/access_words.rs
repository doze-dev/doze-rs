//! The words of the Access page: the state it is in, the three start-up warnings and whether each holds now, and the
//! sentences under the switches. Pure functions of what the page already read; nothing here asks the broker anything.

use crate::model::{AccessWarning, Span};

/// What the warnings are decided from.
#[derive(Debug, Clone, Copy)]
pub struct Switches<'a> {
    pub sasl: bool,
    pub tls: bool,
    pub default_allow: bool,
    /// ACLs are stored.
    pub acls_stored: bool,
    /// The Kafka listener can be reached from other machines.
    pub listener_beyond_loopback: bool,
    /// The console can be reached from other machines.
    pub console_beyond_loopback: bool,
    /// `--console-unprotected`: without it, a console beyond loopback answers other machines only while nothing guards the broker
    /// (SEC-250).
    pub console_unprotected: bool,
    pub console_addr: &'a str,
}

fn plain(text: &str) -> Vec<Span> {
    vec![Span::text(text)]
}

/// The three warnings the broker says at start (SEC-188, SEC-189, SEC-191), each with whether its condition holds.
#[must_use]
pub fn warnings(s: &Switches<'_>) -> Vec<AccessWarning> {
    // What keeps a Kafka client out that the console (no login, plain HTTP, a super user) would get round (SEC-188).
    let protected =
        s.sasl || s.tls || !s.default_allow || s.acls_stored || !s.listener_beyond_loopback;
    // Without --console-unprotected, only ACLs made after the start can guard the broker of a console beyond loopback, and the
    // console then answers this machine alone (SEC-250): the door is shut, not wider.
    let shut = s.console_beyond_loopback && protected && !s.console_unprotected;
    let one = AccessWarning {
        title: plain("Console served beyond loopback, a wider door than the broker"),
        because: if shut {
            vec![
                Span::text("The console is on "),
                Span::mono(s.console_addr),
                Span::text(
                    ", and this broker now has ACLs, so it answers this machine only; restart with ",
                ),
                Span::mono("--console-unprotected"),
                Span::text(" to let other machines in"),
            ]
        } else if s.console_beyond_loopback && protected {
            vec![
                Span::text("The console is on "),
                Span::mono(s.console_addr),
                Span::text(
                    ": it has no login and is plain HTTP, and gets round the broker's SASL, TLS, ACLs or loopback listener; this was accepted with ",
                ),
                Span::mono("--console-unprotected"),
            ]
        } else if s.console_beyond_loopback {
            vec![
                Span::text("The console is on "),
                Span::mono(s.console_addr),
                Span::text(
                    ", and the broker has no SASL, TLS or ACL and listens beyond loopback too",
                ),
            ]
        } else {
            vec![Span::text("The console is on "), Span::mono(s.console_addr)]
        },
        holds: s.console_beyond_loopback && protected && !shut,
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

#[cfg(test)]
mod tests;
