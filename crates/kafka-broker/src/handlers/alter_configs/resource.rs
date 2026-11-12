//! Changing the configs of one resource, as Apache Kafka 4.3.1 does (source: `ConfigAdminManager.preprocess`,
//! `ControllerConfigurationValidator`, `ConfigurationControlManager`): the same for the legacy AlterConfigs, which
//! replaces what a resource has, and for IncrementalAlterConfigs, which changes it.

use std::collections::HashMap;

use kafka_auth::authz::{operation, resource};
use kafka_meta::Topic;
use kafka_storage::fs::Fs;
use kafka_wire::ResponseError as E;
use kafka_wire::ids::TopicName;

use crate::configs::{self, Change, Refusal};
use crate::guard::{CLUSTER_DENIED, GROUP_DENIED, TOPIC_DENIED};
use crate::handlers::create_topics::name_message;
use crate::{Broker, Caller, blocking};

pub(crate) const TOPIC: i8 = 2;
pub(crate) const BROKER: i8 = 4;
pub(crate) const BROKER_LOGGER: i8 = 8;
pub(crate) const CLIENT_METRICS: i8 = 16;
pub(crate) const GROUP: i8 = 32;

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

/// An operation number IncrementalAlterConfigs does not have, if the request gave one.
fn bad_operation(r: &Resource) -> Result<(), Refusal> {
    match r.bad_op {
        Some(op) => Err(refuse(
            E::InvalidRequest,
            format!("Unknown operations type {op}"),
        )),
        None => Ok(()),
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
        caller: &Caller,
        resources: &[Resource],
        validate_only: bool,
        replace: bool,
    ) -> Vec<Option<Refusal>> {
        let _one_change_at_a_time = self.config_admin.lock().await;
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
                self.alter_one(caller, r, validate_only, replace)
                    .await
                    .err()
            });
        }
        out
    }

    /// One resource, in Kafka's order (`ConfigAdminManager.preprocess`, then `ControllerApis`): what is wrong with the
    /// request itself (duplicate keys, null values, and for a broker its name and its configs) is said before the
    /// caller's rights are looked at, so a caller without them sees those first; then ALTER_CONFIGS; then the topic.
    async fn alter_one(
        &self,
        caller: &Caller,
        r: &Resource,
        validate_only: bool,
        replace: bool,
    ) -> Result<(), Refusal> {
        configs::check_request(&r.changes)?;
        match r.kind {
            TOPIC => {
                self.alter_access(caller, r, replace)?;
                bad_operation(r)?;
                self.alter_topic(r, validate_only, replace).await
            }
            BROKER => {
                bad_operation(r)?;
                self.alter_broker(caller, r, validate_only, replace).await
            }
            other => {
                self.alter_access(caller, r, replace)?;
                Err(unsupported(other))
            }
        }
    }

    /// Whether the caller may ALTER_CONFIGS the resource: on the topic; on the cluster for a broker, its client
    /// metrics (and, in IncrementalAlterConfigs only, its loggers); on the group. Any other type is nobody's to
    /// refuse here.
    fn alter_access(&self, caller: &Caller, r: &Resource, replace: bool) -> Result<(), Refusal> {
        let ask = |kind: i8| {
            self.authz
                .allows(caller, kind, &r.name, operation::ALTER_CONFIGS)
        };
        let cluster = || self.cluster_allows(caller, operation::ALTER_CONFIGS);
        let (allowed, error, message) = match r.kind {
            TOPIC => (
                ask(resource::TOPIC),
                E::TopicAuthorizationFailed,
                TOPIC_DENIED,
            ),
            BROKER | CLIENT_METRICS => (cluster(), E::ClusterAuthorizationFailed, CLUSTER_DENIED),
            BROKER_LOGGER if !replace => (cluster(), E::ClusterAuthorizationFailed, CLUSTER_DENIED),
            GROUP => (
                ask(resource::GROUP),
                E::GroupAuthorizationFailed,
                GROUP_DENIED,
            ),
            _ => return Ok(()),
        };
        allowed.then_some(()).ok_or_else(|| refuse(error, message))
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
        caller: &Caller,
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
        // After the name and the configs are found valid, as Kafka does: that is the broker's own preprocessing.
        self.alter_access(caller, r, replace)?;
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
