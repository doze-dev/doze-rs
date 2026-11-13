//! The words of the Access page: the state it is in, the three start-up warnings and whether each holds now, and the
//! sentences under the switches. Pure functions of what the page already read; nothing here asks the broker anything.

use crate::model::{AccessWarning, Span};

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

#[cfg(test)]
mod tests;
