//! What the Kafka major sets in the broker, and the delegation-token flags.

use std::time::Duration;

use anyhow::{Result, bail};
use kafka_broker::{Config as BrokerConfig, TxnVersion};
use kafka_wire::profile::{KafkaMajor, Profile};

use super::Args;

/// How the broker ends a transaction on its own at a Kafka major: the `transaction.version` its profile finalizes.
fn transaction_version(major: KafkaMajor) -> TxnVersion {
    if Profile::new(major).transaction_version() >= 2 {
        TxnVersion::EpochBump
    } else {
        TxnVersion::Classic
    }
}

impl Args {
    /// What the Kafka major sets in the broker (its `transaction.version`, what UpdateFeatures holds a level to), and the
    /// delegation-token flags.
    pub(super) fn major_and_tokens(
        &self,
        broker: &mut BrokerConfig,
        major: KafkaMajor,
        env: &dyn Fn(&str) -> Option<std::ffi::OsString>,
    ) -> Result<()> {
        broker.transaction_version = transaction_version(major);
        broker.kafka_major = major;
        broker.delegation_tokens = self.token_config(env)?;
        Ok(())
    }

    /// The delegation-token flags: the key from the flag, else from the variable; the times in milliseconds, at least 1.
    fn token_config(
        &self,
        env: &dyn Fn(&str) -> Option<std::ffi::OsString>,
    ) -> Result<kafka_broker::TokenConfig> {
        let key = match &self.delegation_token_secret_key {
            Some(key) => Some(key.clone()),
            None => env("DOZE_KAFKA_DELEGATION_TOKEN_SECRET_KEY")
                .map(|v| v.to_string_lossy().into_owned()),
        };
        if key.as_ref().is_some_and(String::is_empty) {
            bail!("the delegation token secret key is empty");
        }
        let ms = |d: Duration, flag: &str| -> Result<i64> {
            let ms = i64::try_from(d.as_millis()).unwrap_or(i64::MAX);
            if ms < 1 {
                bail!("--{flag} must be at least 1ms");
            }
            Ok(ms)
        };
        ms(
            self.delegation_token_expiry_check_interval,
            "delegation-token-expiry-check-interval",
        )?;
        Ok(kafka_broker::TokenConfig {
            secret: key.map(|k| std::sync::Arc::new(kafka_auth::Secret::new(k.into_bytes()))),
            max_lifetime_ms: ms(
                self.delegation_token_max_lifetime,
                "delegation-token-max-lifetime",
            )?,
            expiry_time_ms: ms(
                self.delegation_token_expiry_time,
                "delegation-token-expiry-time",
            )?,
            expiry_check_every: self.delegation_token_expiry_check_interval,
        })
    }
}
