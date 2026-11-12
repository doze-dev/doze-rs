//! The console's flags and the start-up rule on them.
//!
//! `--console`, `--console-addr`, `--console-prefix`, `--console-allow-host` and `--console-unprotected` are parsed and
//! checked here and acted on by `web.rs` (a build with the `console` feature serves a console; one without says the
//! flags are ignored). The rule on their inputs: with SASL or TLS on, a console address that other machines can reach
//! is refused unless `--console-unprotected` accepts it (the console has no login, is plain HTTP and would act as a
//! super user), and the choices that leave something open are reported once at start. The words are the Go edition's.

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
}

impl Default for Console {
    fn default() -> Self {
        Self {
            enabled: true,
            addr: None,
            prefix: "/".to_owned(),
            allow_hosts: Vec::new(),
            unprotected: false,
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
/// `--console` that is not a boolean; an address that is not `host:port`; or SASL or TLS is on and the console would be served beyond loopback
/// without `--console-unprotected`.
pub(super) fn setup(input: &Input<'_>) -> Result<(Console, Vec<String>)> {
    let on = match input.console {
        None => true,
        Some(text) => switch(text).ok_or_else(|| {
            anyhow::anyhow!(
                "config: --console takes true, false, 1, 0, yes, no, on or off, not {text:?}; the console stays on only when it is asked for plainly"
            )
        })?,
    };
    let protection = match (input.sasl, input.tls) {
        (true, true) => Some("SASL and TLS"),
        (true, false) => Some("SASL"),
        (false, true) => Some("TLS"),
        (false, false) => None,
    };
    let mut notes = Vec::new();
    if let Some(addr) = input.addr.filter(|_| on) {
        if !host_port(addr) {
            bail!("config: console address {addr:?} is not host:port (for example 127.0.0.1:9093)");
        }
        if let (Some(what), true) = (protection, beyond_loopback(addr)) {
            let exposure = if input.sasl {
                "let anyone who can reach that address act on the broker as a super user, without a password"
            } else {
                "send topics and messages unencrypted to anyone who can reach that address"
            };
            if !input.unprotected {
                bail!(
                    "config: the console has no login and is plain HTTP, and {what} is on: serving it on {addr} would {exposure}. Keep --console-addr on loopback (and put a proxy that authenticates in front of it), turn it off with --console=false, or accept this with --console-unprotected"
                );
            }
            notes.push(format!(
                "the console is served on {addr} with no login and over plain HTTP although {what} is on (--console-unprotected): that will {exposure}"
            ));
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
    let console = Console {
        enabled: on,
        addr: input.addr.map(str::to_owned),
        prefix: input.prefix.to_owned(),
        allow_hosts,
        unprotected: input.unprotected,
    };
    Ok((console, notes))
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
