//! What the command line comes to once it has been checked: the configuration of the broker, the server, the console and
//! the registry, and the warnings of what is left open.

use anyhow::{Result, bail};
use kafka_broker::Config as BrokerConfig;
use kafka_server::{Config as ServerConfig, Timeouts};
use kafka_wire::profile::{KafkaMajor, Profile};

use super::console_flags::Guards;
use super::{Args, Console, Registry, registry, tls};
use crate::duration;

/// What the command line comes to once it has been checked.
#[derive(Debug)]
pub(crate) struct Settings {
    pub(crate) broker: BrokerConfig,
    pub(crate) server: ServerConfig,
    /// The console flags (`web.rs` serves the console they describe).
    pub(crate) console: Console,
    /// The schema registry's flags (`registry.rs` serves the registry they describe).
    #[cfg_attr(not(feature = "registry"), allow(dead_code))]
    pub(crate) registry: Registry,
    /// What the operator chose that leaves something open, to be said once at start.
    pub(crate) warnings: Vec<String>,
}

impl Args {
    /// Checks the arguments and turns them into the configuration of the broker and the server.
    pub(crate) fn settings(&self) -> Result<Settings> {
        self.settings_in(&|name| std::env::var_os(name))
    }

    /// [`Args::settings`] with the environment given, so that a test need not change the process's own.
    pub(crate) fn settings_in(
        &self,
        env: &dyn Fn(&str) -> Option<std::ffi::OsString>,
    ) -> Result<Settings> {
        // Without the feature this refuses the flags, and a value that is `()` is all that is left of them.
        #[cfg_attr(not(feature = "tls"), allow(clippy::let_unit_value, unused_variables))]
        let tls = tls::load(self.tls_cert.as_ref(), self.tls_key.as_ref())?;
        // Both flags are set or neither (and without the feature neither is accepted).
        let tls_on = self.tls_cert.is_some();
        let Some(major) = KafkaMajor::new(self.kafka_version) else {
            bail!(
                "--kafka-version must be 1, 2, 3 or 4, not {}",
                self.kafka_version
            );
        };
        self.check_numbers()?;
        let mut broker = self.broker(major, env)?;
        let sasl = self.login.sasl(env, &self.data_dir)?;
        broker.users = sasl.users;
        let (console, registry, mut warnings) =
            self.web_rules(sasl.authenticator.is_some(), tls_on)?;
        warnings.extend(tls::key_warning(self.tls_key.as_ref()));
        let server = ServerConfig {
            sasl: sasl.authenticator,
            #[cfg(feature = "tls")]
            tls,
            listen: self.listen,
            advertised_host: self.advertised_host.clone(),
            advertised_port: self.advertised_port,
            profile: Profile::new(major),
            timeouts: Timeouts {
                failed_login: self.login.sasl_failed_login_delay,
                ..Timeouts::default()
            },
            ..ServerConfig::default()
        };
        Ok(Settings {
            broker,
            server,
            console,
            registry,
            warnings,
        })
    }

    /// The broker's configuration (its users are added by the caller).
    fn broker(
        &self,
        major: KafkaMajor,
        env: &dyn Fn(&str) -> Option<std::ffi::OsString>,
    ) -> Result<BrokerConfig> {
        let mut broker = BrokerConfig::new(self.data_dir.clone());
        broker.auto_create_topics = self.auto_create_topics;
        broker.default_partitions = self.default_partitions;
        broker.max_partitions = self.max_partitions;
        broker.fsync_interval = self.fsync_interval;
        broker.group_initial_rebalance_delay = self.group_initial_rebalance_delay;
        broker.consumer_session_timeout = self.consumer_session_timeout;
        // A test knob no flag covers: the least time between two computations of a consumer group's assignment.
        if let Some(v) = env("DOZE_KAFKA_TEST_ASSIGNMENT_INTERVAL") {
            broker.consumer_assignment_interval = duration::parse(&v.to_string_lossy())
                .map_err(|e| anyhow::anyhow!("DOZE_KAFKA_TEST_ASSIGNMENT_INTERVAL: {e}"))?;
        }
        broker.offsets_retention = self.offsets_retention;
        broker.log_retention_check_every = self.log_retention_check_interval;
        broker.log_cleaner_every = self.log_cleaner_interval;
        broker.log_cleaner_enable = self.log_cleaner_enable;
        broker.client_telemetry = self.client_telemetry;
        broker.unstable_api_versions = self.unstable_api_versions;
        broker.transaction_two_phase_commit = self.transaction_two_phase_commit;
        self.major_and_tokens(&mut broker, major, env)?;
        broker.max_open_partitions = self.max_open_partitions;
        broker.acl_default_allow = self.login.acl_allow();
        broker.super_users = self.login.super_users()?;
        broker.defaults = self.topic_defaults();
        Ok(broker)
    }

    /// The console's and the schema registry's flags, and what they leave open.
    fn web_rules(&self, sasl: bool, tls: bool) -> Result<(Console, Registry, Vec<String>)> {
        let (console, mut warnings) = self.console.rule(&Guards {
            sasl,
            tls,
            listen: self.listen,
            acl_allow: self.login.acl_allow(),
        })?;
        let names: Vec<String> = self
            .console
            .console_allow_host
            .iter()
            .chain(&self.advertised_host)
            .cloned()
            .collect();
        let (registry, notes) = registry::setup(&self.registry, self.listen, tls, sasl, &names)?;
        warnings.extend(notes);
        Ok((console, registry, warnings))
    }

    /// The flags of later parts that were given a value and are being ignored, for a warning. A build without the
    /// `console` feature has no console, so it names the console flags that were given.
    pub(crate) fn ignored(&self) -> Vec<&'static str> {
        let mut out = self.registry_ignored();
        if !cfg!(feature = "console") {
            out.extend(self.console.given());
        }
        out
    }

    /// The registry flags that were given, in a build without the `registry` feature.
    fn registry_ignored(&self) -> Vec<&'static str> {
        if cfg!(feature = "registry") {
            return Vec::new();
        }
        let r = &self.registry;
        [
            (r.schema_registry_addr.is_some(), "--schema-registry-addr"),
            (!r.schema_registry_tls, "--schema-registry-tls"),
        ]
        .into_iter()
        .filter(|(on, _)| *on)
        .map(|(_, name)| name)
        .collect()
    }
}
