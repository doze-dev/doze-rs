//! Changing the configs of one resource, as Apache Kafka 4.3.1 does (source: `ConfigAdminManager.preprocess`,
//! `ControllerConfigurationValidator`, `ConfigurationControlManager`): the same for the legacy AlterConfigs, which
//! replaces what a resource has, and for IncrementalAlterConfigs, which changes it.

use std::collections::HashMap;

use kafka_meta::Topic;
use kafka_storage::fs::Fs;
use kafka_wire::ResponseError as E;
use kafka_wire::ids::TopicName;

use crate::configs::{self, Change, Refusal};
use crate::handlers::create_topics::name_message;
use crate::{Broker, blocking};

pub(crate) const TOPIC: i8 = 2;
pub(crate) const BROKER: i8 = 4;

/// What a request asks of one resource.
#[derive(Debug, Clone)]
pub(super) struct Resource {
    pub(super) kind: i8,
    pub(super) name: String,
    pub(super) changes: Vec<Change>,
    /// An operation number IncrementalAlterConfigs does not have, if one was given.
    pub(super) bad_op: Option<i8>,
}

fn refuse(code: E, message: impl Into<String>) -> Refusal {
    Refusal {
        code,
        message: message.into(),
    }
}

/// What Kafka says of a resource type this broker does not take, or does not know.
pub(crate) fn unsupported(kind: i8) -> Refusal {
    let name = match kind {
        8 => "BROKER_LOGGER",
        16 => "CLIENT_METRICS",
        32 => "GROUP",
        _ => return refuse(E::InvalidRequest, format!("Unknown resource type {kind}")),
    };
    refuse(
        E::InvalidRequest,
        format!("Unsupported resource type: {name}"),
    )
}

impl<F: Fs> Broker<F> {
    /// Each resource, in the order given: `None` if it was changed (or, with `validate_only`, could be), else why not.
    /// `replace` is the legacy request: the configs given are all the resource is to have.
    pub(super) async fn alter_resources(
        &self,
        resources: &[Resource],
        validate_only: bool,
        replace: bool,
    ) -> Vec<Option<Refusal>> {
        let mut count: HashMap<(i8, &str), usize> = HashMap::new();
        for r in resources {
            *count.entry((r.kind, r.name.as_str())).or_default() += 1;
        }
        let mut out = Vec::with_capacity(resources.len());
        for r in resources {
            let twice = count
                .get(&(r.kind, r.name.as_str()))
                .is_some_and(|n| *n > 1);
            out.push(if twice {
                Some(refuse(
                    E::InvalidRequest,
                    "Each resource must appear at most once.",
                ))
            } else {
                self.alter_one(r, validate_only, replace).await.err()
            });
        }
        out
    }

    async fn alter_one(
        &self,
        r: &Resource,
        validate_only: bool,
        replace: bool,
    ) -> Result<(), Refusal> {
        configs::check_request(&r.changes)?;
        if let Some(op) = r.bad_op {
            return Err(refuse(
                E::InvalidRequest,
                format!("Unknown operations type {op}"),
            ));
        }
        match r.kind {
            TOPIC => self.alter_topic(r, validate_only, replace).await,
            BROKER => self.alter_broker(r, validate_only, replace).await,
            other => Err(unsupported(other)),
        }
    }

    async fn alter_topic(
        &self,
        r: &Resource,
        validate_only: bool,
        replace: bool,
    ) -> Result<(), Refusal> {
        if r.name.is_empty() {
            return Err(refuse(
                E::InvalidRequest,
                "Default topic resources are not allowed.",
            ));
        }
        let name = TopicName::parse(&r.name)
            .map_err(|e| refuse(E::InvalidTopicException, name_message(e, &r.name)))?;
        let existing = self.meta.topic(&name);
        let current = existing
            .as_ref()
            .filter(|_| !replace)
            .map(|t| t.config.clone())
            .unwrap_or_default();
        // The configs are judged before the topic's absence is.
        let next = configs::apply(&self.defaults.get(), &current, &r.changes)?;
        let missing = || {
            refuse(
                E::UnknownTopicOrPartition,
                format!("The topic '{}' does not exist.", r.name),
            )
        };
        if existing.is_none() {
            return Err(missing());
        }
        if validate_only {
            return Ok(());
        }
        let changed = blocking::run(|| {
            self.meta
                .update_topic(&name, |t| Topic {
                    config: next.clone(),
                    ..t.clone()
                })
        })
        .map_err(|e| {
            tracing::error!(topic = %r.name, error = %e, "configs: could not store a topic's configs");
            refuse(E::UnknownServerError, "The configs could not be stored.")
        })?
        .ok_or_else(missing)?;
        self.partitions.reconfigure(&[changed]).await;
        Ok(())
    }

    async fn alter_broker(
        &self,
        r: &Resource,
        validate_only: bool,
        replace: bool,
    ) -> Result<(), Refusal> {
        let node = self.config.node_id;
        if !r.name.is_empty() {
            let id: i32 = r.name.parse().map_err(|_| {
                refuse(
                    E::InvalidRequest,
                    format!("Node id must be an integer, but it is: {}", r.name),
                )
            })?;
            if id != node {
                return Err(refuse(
                    E::InvalidRequest,
                    format!(
                        "Unexpected broker id, expected {node}, but received {}",
                        r.name
                    ),
                ));
            }
        }
        let mut dynamic = self.defaults.dynamic();
        let scope = if r.name.is_empty() {
            &mut dynamic.cluster
        } else {
            &mut dynamic.own
        };
        let current = if replace {
            Default::default()
        } else {
            scope.clone()
        };
        *scope = configs::apply_to_broker(&current, &r.changes)?;
        if validate_only {
            return Ok(());
        }
        let key = if r.name.is_empty() {
            String::new()
        } else {
            node.to_string()
        };
        let stored = if r.name.is_empty() {
            &dynamic.cluster
        } else {
            &dynamic.own
        };
        blocking::run(|| self.meta.set_broker_configs(&key, stored)).map_err(|e| {
            tracing::error!(error = %e, "configs: could not store the broker's configs");
            refuse(E::UnknownServerError, "The configs could not be stored.")
        })?;
        self.defaults.set(dynamic);
        self.partitions.reconfigure(&self.meta.topics()).await;
        Ok(())
    }
}
