//! The console's flags as clap reads them (flattened into `Args`); `console.rs` holds the start-up rule on them.

use std::net::SocketAddr;

use anyhow::Result;

use super::console::{self, Console};

/// `--console`, `--console-addr`, `--console-prefix`, `--console-allow-host`, `--console-unprotected` and `--lab`.
#[derive(Debug, clap::Args)]
pub(crate) struct ConsoleFlags {
    /// The web console: on unless `--console=false`. It listens beside the broker, in the same process, and shows what the
    /// broker holds. It has no login and acts as a super user: on a shared machine any local process can use it without the
    /// broker's password or past its ACLs (it is a tool for one developer), so turn it off there with `--console=false`. A
    /// build without the `console` feature accepts the flag and serves none.
    #[arg(long, num_args = 0..=1, default_missing_value = "true", value_name = "BOOL")]
    console: Option<String>,
    /// Where the console listens (default 127.0.0.1:9093: the Kafka port plus one, on this machine only). If the default
    /// port is busy the broker runs without a console and says so; an address you give that cannot be bound stops the start.
    #[arg(long, value_name = "HOST:PORT")]
    console_addr: Option<String>,
    /// The path the console is served under, for a proxy that forwards it as it is (default `/`; every link follows it).
    #[arg(long, value_name = "PATH", default_value = "/")]
    console_prefix: String,
    /// A host name the console answers to besides localhost and IP addresses (repeatable, or comma separated; `*`
    /// turns the check off). It guards against DNS rebinding: a request for any other `Host` is refused with 403.
    #[arg(long, value_name = "HOST", value_delimiter = ',')]
    pub(crate) console_allow_host: Vec<String>,
    /// Accept serving the console beyond loopback although that makes it a wider door than the broker: SASL or TLS is on, ACLs
    /// are in force (--acl-default deny, or ACLs stored), or --listen keeps Kafka on loopback. The console has no login, is plain
    /// HTTP and acts as a super user, so without this that start is refused. Every page then says it is served beyond loopback.
    /// Without it, a console beyond loopback in front of an open broker stops answering other machines while the broker has
    /// any ACL (made after the start, over Kafka or from the console), and answers them again once none is left.
    #[arg(long)]
    console_unprotected: bool,
    /// The Lab of the console: rules that delay, fail or drop requests on purpose, made from the console only and kept in memory.
    /// On unless `--lab=false`; off when the console is off, and off by default when the console is served beyond loopback
    /// (`--lab=true` switches it on there). Anyone who can reach the console can inject faults.
    #[arg(long, num_args = 0..=1, default_missing_value = "true", value_name = "BOOL")]
    lab: Option<String>,
}

/// What protects the broker, which the console's start-up rule weighs.
pub(crate) struct Guards {
    pub(crate) sasl: bool,
    pub(crate) tls: bool,
    pub(crate) listen: SocketAddr,
    pub(crate) acl_allow: bool,
}

impl ConsoleFlags {
    /// The console flags, and the start-up rule between them and what protects the broker.
    pub(crate) fn rule(&self, g: &Guards) -> Result<(Console, Vec<String>)> {
        console::setup(&console::Input {
            console: self.console.as_deref(),
            addr: self.console_addr.as_deref(),
            prefix: &self.console_prefix,
            allow_hosts: &self.console_allow_host,
            unprotected: self.console_unprotected,
            lab: self.lab.as_deref(),
            sasl: g.sasl,
            tls: g.tls,
            listen: g.listen,
            acl_allow: g.acl_allow,
        })
    }

    /// The console flags that were given, for the warning of a build without the `console` feature.
    pub(crate) fn given(&self) -> Vec<&'static str> {
        [
            (
                self.console
                    .as_deref()
                    .is_some_and(|v| console::switch(v) != Some(false)),
                "--console",
            ),
            (self.console_addr.is_some(), "--console-addr"),
            (self.console_prefix != "/", "--console-prefix"),
            (!self.console_allow_host.is_empty(), "--console-allow-host"),
            (self.console_unprotected, "--console-unprotected"),
            (self.lab.is_some(), "--lab"),
        ]
        .into_iter()
        .filter(|(on, _)| *on)
        .map(|(_, name)| name)
        .collect()
    }
}
