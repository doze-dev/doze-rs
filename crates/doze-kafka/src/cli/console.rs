//! The console's flags and the start-up rule on them.
//!
//! `--console`, `--console-addr`, `--console-prefix`, `--console-allow-host` and `--console-unprotected` are parsed and
//! checked here and acted on by `web.rs` (a build with the `console` feature serves a console; one without says the
//! flags are ignored). The rule on their inputs: the console is never a wider door than the broker itself. With SASL or
//! TLS on, ACLs in force, or Kafka listening on loopback only, a console address that other machines can reach is refused
//! unless `--console-unprotected` accepts it (the console has no login, is plain HTTP and acts as a super user), and the
//! choices that leave something open are reported once at start. ACLs made after the start are a guard too: the console holds
//! the same rule for them while it runs (`kafka_console::door`, SEC-250).

use std::net::{IpAddr, SocketAddr};

use anyhow::{Result, bail};

/// What the console flags come to.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Console {
    /// `--console`: false only for `--console=false`.
    pub(crate) enabled: bool,
    /// `--console-addr`, if given (the default, `127.0.0.1:9093`, is not an address the operator chose).
    pub(crate) addr: Option<String>,
    /// `--console-prefix`: the path the console is served under (`/` is the root).
    pub(crate) prefix: String,
    /// `--console-allow-host`: the names, besides loopback ones and addresses, the console answers to (`*` = all).
    pub(crate) allow_hosts: Vec<String>,
    /// `--console-unprotected`.
    pub(crate) unprotected: bool,
    /// The Lab is on: `--lab` (the default) with a console to make rules from, and not the default-off of a console served
    /// beyond loopback.
    pub(crate) lab: bool,
}

impl Default for Console {
    fn default() -> Self {
        Self {
            enabled: true,
            addr: None,
            prefix: "/".to_owned(),
            allow_hosts: Vec::new(),
            unprotected: false,
            lab: true,
        }
    }
}

#[cfg_attr(not(feature = "console"), allow(dead_code))]
/// The address the console listens on when `--console-addr` is not given: the Kafka port plus one, on loopback.
pub(crate) const DEFAULT_ADDR: &str = "127.0.0.1:9093";

/// What the rule looks at.
pub(super) struct Input<'a> {
    /// `--console`: a boolean (see [`switch`]); absent is on.
    pub(super) console: Option<&'a str>,
    pub(super) addr: Option<&'a str>,
    pub(super) prefix: &'a str,
    pub(super) allow_hosts: &'a [String],
    pub(super) unprotected: bool,
    /// `--lab`: a boolean; absent is on, unless the console is served beyond loopback.
    pub(super) lab: Option<&'a str>,
    pub(super) sasl: bool,
    pub(super) tls: bool,
    pub(super) listen: SocketAddr,
    /// `--acl-default allow`: what no ACL covers is allowed.
    pub(super) acl_allow: bool,
}

/// SASL on with `--acl-default allow` (SEC-191): a login proves who a client is and then allows it everything.
pub(super) const OPEN_ACLS: &str = "SASL is on and --acl-default is allow, so every logged-in user may do everything, including managing users: use --acl-default deny with ACLs, or --acl-super-user";

/// A boolean as `--console` is written: `true`, `false`, `1`, `0`, `yes`, `no`, `on` or `off`, in any case. Anything else is
/// not a boolean: a typo must not leave a console with no login open when the person meant to turn it off.
pub(super) fn switch(text: &str) -> Option<bool> {
    match text.to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Some(true),
        "false" | "0" | "no" | "off" => Some(false),
        _ => None,
    }
}

/// Checks the console flags against what protects the broker, and says what is left open.
///
/// # Errors
/// `--console` that is not a boolean; an address that is not `host:port`; or the console would be served beyond loopback without
/// `--console-unprotected` while SASL, TLS or ACLs guard the broker or Kafka listens on loopback only.
pub(super) fn setup(input: &Input<'_>) -> Result<(Console, Vec<String>)> {
    let on = match input.console {
        None => true,
        Some(text) => switch(text).ok_or_else(|| {
            anyhow::anyhow!(
                "config: --console takes true, false, 1, 0, yes, no, on or off, not {text:?}; the console stays on only when it is asked for plainly"
            )
        })?,
    };
    let mut notes = Vec::new();
    if let Some(addr) = input.addr.filter(|_| on) {
        if !host_port(addr) {
            bail!("config: console address {addr:?} is not host:port (for example 127.0.0.1:9093)");
        }
        if beyond_loopback(addr) {
            let guards = Guards::of(input);
            if let Some(note) = wider_door(addr, &guards, input.unprotected)? {
                notes.push(note);
            } else {
                notes.push(open_broker_note(addr, input.unprotected));
            }
        }
    }
    if input.sasl && !input.tls && !input.listen.ip().is_loopback() {
        notes.push(format!(
            "SASL is on without TLS and --listen {} can be reached from other machines: logins (a PLAIN password is sent as it is) and messages cross the network unencrypted",
            input.listen
        ));
    }
    if input.sasl && input.acl_allow {
        notes.push(OPEN_ACLS.to_owned());
    }
    let allow_hosts = input
        .allow_hosts
        .iter()
        .map(|h| h.trim().to_owned())
        .filter(|h| !h.is_empty())
        .collect();
    if on {
        check_prefix(input.prefix)?;
    }
    let lab = lab_rule(input, on, &mut notes)?;
    let console = Console {
        enabled: on,
        addr: input.addr.map(str::to_owned),
        prefix: input.prefix.to_owned(),
        allow_hosts,
        unprotected: input.unprotected,
        lab,
    };
    Ok((console, notes))
}

/// The note for a console beyond loopback in front of a broker nothing guards (SEC-189). ACLs made later are a guard the start
/// cannot see: without `--console-unprotected` the console stops answering other machines while there are any (SEC-250); with it,
/// they do not bind the console.
fn open_broker_note(addr: &str, unprotected: bool) -> String {
    let later = if unprotected {
        "ACLs made later will not bind the console, which acts as a super user (--console-unprotected)"
    } else {
        "if ACLs are made later, the console stops answering other machines while there are any (it acts as a super user and would get round them; --console-unprotected keeps it open)"
    };
    format!(
        "the console is served on {addr} with no login and over plain HTTP: anyone who can reach that address can do what any Kafka client of this broker can (it has no login, no ACL and no TLS); {later}"
    )
}

/// What keeps a Kafka client of this broker out that a console served beyond loopback would get round: the console has no login,
/// is plain HTTP and acts as a super user (SEC-241), and it must never be a wider door than the broker itself (SEC-243).
#[derive(Debug, Default)]
struct Guards {
    sasl: bool,
    tls: bool,
    /// Why ACLs are in force, if they are: `--acl-default deny`, or ACLs stored in the data directory.
    acls: Option<String>,
    /// The Kafka address, if only this machine can reach it.
    local_listen: Option<SocketAddr>,
}

impl Guards {
    fn of(input: &Input<'_>) -> Self {
        Self {
            sasl: input.sasl,
            tls: input.tls,
            acls: (!input.acl_allow).then(|| "ACLs are in force (--acl-default deny)".to_owned()),
            local_listen: input.listen.ip().is_loopback().then_some(input.listen),
        }
    }

    /// What is on, in words: `SASL is on, TLS is on and ...`; `None` when nothing guards the broker.
    fn what(&self) -> Option<String> {
        let mut said = Vec::new();
        if self.sasl {
            said.push("SASL is on".to_owned());
        }
        if self.tls {
            said.push("TLS is on".to_owned());
        }
        if let Some(acls) = &self.acls {
            said.push(acls.clone());
        }
        if let Some(listen) = self.local_listen {
            said.push(format!("--listen {listen} keeps Kafka on this machine"));
        }
        (!said.is_empty()).then(|| listed(&said))
    }

    /// What anyone who can reach the console could then do that a Kafka client could not.
    fn exposure(&self) -> String {
        let mut said = Vec::new();
        if self.sasl {
            said.push("act on the broker as a super user without a password");
        }
        if self.acls.is_some() {
            said.push("do what the ACLs refuse (the console acts as a super user)");
        }
        if self.tls {
            said.push("read topics and messages sent unencrypted");
        }
        if self.local_listen.is_some() {
            said.push("reach from the network a broker that Kafka clients can reach only from this machine");
        }
        listed(&said)
    }
}

/// `a`, `a and b`, `a, b and c`.
fn listed<S: AsRef<str>>(items: &[S]) -> String {
    match items {
        [] => String::new(),
        [one] => one.as_ref().to_owned(),
        [rest @ .., last] => format!(
            "{} and {}",
            rest.iter()
                .map(AsRef::as_ref)
                .collect::<Vec<_>>()
                .join(", "),
            last.as_ref()
        ),
    }
}

/// The rule for a console served on `addr`, beyond loopback: refused when something guards the broker unless `--console-unprotected`
/// accepts it, and then the note that says what is open. `None` when nothing guards the broker.
fn wider_door(addr: &str, guards: &Guards, unprotected: bool) -> Result<Option<String>> {
    let Some(what) = guards.what() else {
        return Ok(None);
    };
    let exposure = guards.exposure();
    if !unprotected {
        bail!(
            "config: the console has no login and is plain HTTP, and {what}: serving it on {addr} would let anyone who can reach that address {exposure}. Keep --console-addr on loopback (and put a proxy that authenticates in front of it), turn it off with --console=false, or accept this with --console-unprotected"
        );
    }
    Ok(Some(format!(
        "the console is served on {addr} with no login and over plain HTTP although {what} (--console-unprotected): anyone who can reach that address can {exposure}"
    )))
}

/// The rule once the broker's data is open, for what only the data directory knows: ACLs stored there, with `--acl-default allow`
/// (with deny the start-up rule has already looked). A console served beyond loopback in front of them is refused unless
/// `--console-unprotected` accepts it, and then this is the note.
///
/// # Errors
/// ACLs are stored and the console would be served beyond loopback without `--console-unprotected`.
#[cfg_attr(not(feature = "console"), allow(dead_code))]
pub(crate) fn stored_acls(
    console: &Console,
    acl_default_allow: bool,
    acls: usize,
) -> Result<Option<String>> {
    let Some(addr) = console.addr.as_deref().filter(|_| console.enabled) else {
        return Ok(None);
    };
    if !acl_default_allow || acls == 0 || !beyond_loopback(addr) {
        return Ok(None);
    }
    let guards = Guards {
        acls: Some(format!(
            "ACLs are in force ({acls} stored in the data directory)"
        )),
        ..Guards::default()
    };
    wider_door(addr, &guards, console.unprotected)
}

/// Whether the Lab is on, and what it is said to leave open. The Lab needs the console to make rules from; a console served beyond
/// loopback starts with it off unless `--lab` asks for it plainly (anyone who reaches such a console could inject faults).
fn lab_rule(input: &Input<'_>, console_on: bool, notes: &mut Vec<String>) -> Result<bool> {
    let asked = match input.lab {
        None => None,
        Some(text) => Some(switch(text).ok_or_else(|| {
            anyhow::anyhow!(
                "config: --lab takes true, false, 1, 0, yes, no, on or off, not {text:?}; the Lab stays on only when it is asked for plainly"
            )
        })?),
    };
    let beyond = console_on && input.addr.is_some_and(beyond_loopback);
    let lab = console_on && asked.unwrap_or(!beyond);
    if console_on && beyond && lab {
        notes.push("the Lab is on (--lab) and the console is served beyond loopback: anyone who can reach the console can inject faults into the broker's connections (--lab=false turns it off)".to_owned());
    }
    if console_on && beyond && !lab && asked.is_none() {
        notes.push("the Lab is off because the console is served beyond loopback (--lab=true switches it on)".to_owned());
    }
    Ok(lab)
}

/// Whether `addr` is `host:port` with a numeric port, as `--console-addr` is written (the host may be empty: `:9093`).
fn host_port(addr: &str) -> bool {
    addr.rsplit_once(':').is_some_and(|(host, port)| {
        !port.is_empty()
            && port.len() <= 5
            && port.bytes().all(|b| b.is_ascii_digit())
            && port.parse::<u16>().is_ok()
            && (!host.contains(':') || (host.starts_with('[') && host.ends_with(']')))
    })
}

/// Whether a listener on `addr` (`host:port`) can be reached from another machine: an empty or wildcard host, an
/// address that is not loopback, or a name other than `localhost` (a name is not resolved: one that is not
/// `localhost` is taken at its word).
fn beyond_loopback(addr: &str) -> bool {
    let Some((host, _)) = addr.rsplit_once(':') else {
        return false;
    };
    let host = host
        .strip_prefix('[')
        .and_then(|h| h.strip_suffix(']'))
        .unwrap_or(host);
    match host.parse::<IpAddr>() {
        Ok(ip) => !ip.is_loopback(),
        Err(_) => !host.eq_ignore_ascii_case("localhost"),
    }
}

/// `--console-prefix` is a path: letters, digits, `-`, `_`, `.`, `~` and `/`, no empty step inside it and no `.` or `..`
/// step. A leading and a trailing slash are not significant (`kafka`, `/kafka/` and `/kafka` are one prefix).
fn check_prefix(prefix: &str) -> Result<()> {
    let inner = prefix.trim_matches('/');
    let chars = inner
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~' | b'/'));
    let steps = inner.is_empty()
        || inner
            .split('/')
            .all(|s| !s.is_empty() && s != "." && s != "..");
    let ok = chars && steps;
    if ok {
        Ok(())
    } else {
        bail!(
            "config: console prefix {prefix:?} must be a path of letters, digits, - _ . ~ and / (for example /kafka), with no empty or . or .. step"
        )
    }
}
