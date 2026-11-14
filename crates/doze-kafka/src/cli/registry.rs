//! The schema registry's flags: `--schema-registry`, `--schema-registry-addr` and `--schema-registry-tls`, as the Go
//! edition has them. The registry is on by default; it listens on 127.0.0.1:8081 (Confluent's port), or on a port the
//! system chooses when `--listen` asks for port 0, so that a test's broker cannot collide with another. It asks for the
//! broker's SASL users as HTTP basic credentials, answers to the names the console answers to and to the advertised host,
//! and is served over TLS with the broker's certificate when the broker has one (`--schema-registry-tls=false` keeps
//! plain HTTP). `registry.rs` serves what these come to; a build without the `registry` feature accepts and ignores them.

use std::net::SocketAddr;

use anyhow::{Result, bail};
use clap::Args;

/// The flags, as clap reads them (flattened into the command line).
#[derive(Debug, Clone, Args)]
#[command(next_help_heading = "Schema registry")]
pub(crate) struct RegistryArgs {
    /// Serve a Confluent-compatible schema registry beside the broker (Avro; Protobuf and JSON Schema are coming).
    /// `--schema-registry=false` serves none.
    #[arg(long, default_value_t = true, action = clap::ArgAction::Set, num_args = 0..=1, default_missing_value = "true", value_name = "BOOL")]
    pub(crate) schema_registry: bool,
    /// Where the schema registry listens (default 127.0.0.1:8081, Confluent's port; a free loopback port when --listen is on
    /// port 0). If the default port is busy the broker runs without a registry and says so; an address you give that cannot
    /// be bound stops the start.
    #[arg(long, value_name = "HOST:PORT")]
    pub(crate) schema_registry_addr: Option<String>,
    /// Serve the schema registry over TLS, with the broker's certificate, when --tls-cert and --tls-key are set;
    /// `--schema-registry-tls=false` keeps it plain HTTP.
    #[arg(long, default_value_t = true, action = clap::ArgAction::Set, num_args = 0..=1, default_missing_value = "true", value_name = "BOOL")]
    pub(crate) schema_registry_tls: bool,
}

/// The default address beside a broker on a fixed port.
pub(crate) const DEFAULT_ADDR: &str = "127.0.0.1:8081";
/// The address beside a broker on port 0.
const ANY_PORT: &str = "127.0.0.1:0";

/// What the flags come to.
#[cfg_attr(not(feature = "registry"), allow(dead_code))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Registry {
    pub(crate) enabled: bool,
    /// Where to listen.
    pub(crate) addr: String,
    /// Whether `addr` was given: one that cannot be bound stops the start; the default one is only a warning.
    pub(crate) chosen: bool,
    /// Serve over TLS (the broker has a certificate and `--schema-registry-tls` was not turned off).
    pub(crate) tls: bool,
    /// The names it answers to besides addresses and localhost: the console's, the advertised host, and the host of `addr`
    /// when that is a name.
    pub(crate) allow_hosts: Vec<String>,
}

/// Whether `addr` reaches beyond this machine (not a loopback address or `localhost`).
fn beyond_loopback(addr: &str) -> bool {
    let host = addr.rsplit_once(':').map_or(addr, |(h, _)| h);
    let host = host.trim_start_matches('[').trim_end_matches(']');
    match host.parse::<std::net::IpAddr>() {
        Ok(ip) => !ip.is_loopback(),
        Err(_) => !host.eq_ignore_ascii_case("localhost"),
    }
}

/// Checks the flags and says what they leave open.
///
/// # Errors
/// An address that is not `host:port`.
pub(super) fn setup(
    args: &RegistryArgs,
    listen: SocketAddr,
    tls_on: bool,
    sasl_on: bool,
    names: &[String],
) -> Result<(Registry, Vec<String>)> {
    let chosen = args.schema_registry_addr.is_some();
    let addr = match &args.schema_registry_addr {
        Some(a) => a.trim().to_owned(),
        None if listen.port() == 0 => ANY_PORT.to_owned(),
        None => DEFAULT_ADDR.to_owned(),
    };
    let Some((host, port)) = addr.rsplit_once(':') else {
        bail!(
            "config: --schema-registry-addr {addr:?} is not host:port (for example 127.0.0.1:8081)"
        );
    };
    if args.schema_registry && port.parse::<u16>().is_err() {
        bail!(
            "config: --schema-registry-addr {addr:?} is not host:port (for example 127.0.0.1:8081)"
        );
    }
    let mut allow_hosts: Vec<String> = names
        .iter()
        .map(|h| h.trim().to_owned())
        .filter(|h| !h.is_empty())
        .collect();
    let host = host.trim_start_matches('[').trim_end_matches(']');
    if !host.is_empty() && host.parse::<std::net::IpAddr>().is_err() {
        allow_hosts.push(host.to_owned());
    }
    let tls = tls_on && args.schema_registry_tls;
    let mut notes = Vec::new();
    if args.schema_registry && tls_on && !tls && beyond_loopback(&addr) {
        let what = if sasl_on {
            "its requests, answers and the basic-auth passwords (the broker's SASL users)"
        } else {
            "its requests and answers"
        };
        notes.push(format!(
            "the schema registry is served on {addr} over plain HTTP although the broker has TLS on (--schema-registry-tls=false): {what} cross the network unencrypted"
        ));
    }
    let registry = Registry {
        enabled: args.schema_registry,
        addr,
        chosen,
        tls,
        allow_hosts,
    };
    Ok((registry, notes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(on: bool, addr: Option<&str>, tls: bool) -> RegistryArgs {
        RegistryArgs {
            schema_registry: on,
            schema_registry_addr: addr.map(str::to_owned),
            schema_registry_tls: tls,
        }
    }

    fn listen(s: &str) -> SocketAddr {
        s.parse().unwrap()
    }

    #[test]
    fn the_default_address_is_confluents_port_or_any_port_beside_a_broker_on_port_0() {
        let (r, _) = setup(
            &args(true, None, true),
            listen("127.0.0.1:9092"),
            false,
            false,
            &[],
        )
        .unwrap();
        assert_eq!(
            (r.addr.as_str(), r.chosen, r.tls),
            (DEFAULT_ADDR, false, false)
        );
        let (r, _) = setup(
            &args(true, None, true),
            listen("127.0.0.1:0"),
            false,
            false,
            &[],
        )
        .unwrap();
        assert_eq!(r.addr, ANY_PORT);
        let (r, _) = setup(
            &args(true, Some("devbox.lan:18081"), true),
            listen("127.0.0.1:0"),
            true,
            false,
            &["k.example".into()],
        )
        .unwrap();
        assert_eq!(
            (r.chosen, r.tls, r.allow_hosts.clone()),
            (
                true,
                true,
                vec!["k.example".to_owned(), "devbox.lan".to_owned()]
            )
        );
    }

    #[test]
    fn a_bad_address_is_refused_and_plain_http_beside_tls_is_said() {
        assert!(
            setup(
                &args(true, Some("nope"), true),
                listen("127.0.0.1:9092"),
                false,
                false,
                &[]
            )
            .is_err()
        );
        assert!(
            setup(
                &args(true, Some("h:port"), true),
                listen("127.0.0.1:9092"),
                false,
                false,
                &[]
            )
            .is_err()
        );
        let (r, notes) = setup(
            &args(true, Some("0.0.0.0:8081"), false),
            listen("0.0.0.0:9092"),
            true,
            true,
            &[],
        )
        .unwrap();
        assert!(!r.tls);
        assert!(
            notes
                .first()
                .is_some_and(|n| n.contains("basic-auth passwords")),
            "{notes:?}"
        );
        let (_, quiet) = setup(
            &args(true, None, false),
            listen("127.0.0.1:9092"),
            true,
            true,
            &[],
        )
        .unwrap();
        assert!(quiet.is_empty());
    }
}
