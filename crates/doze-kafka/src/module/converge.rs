//! Keeping the declared topics as declared (converge), naming them for `doze plan` (objects), and the health check.
//!
//! Converge creates a missing topic with its partitions and configs, as the Go module did. Unlike it, a topic that
//! exists is brought to what is declared: fewer partitions than declared are raised, and declared configs that differ
//! are set (the topic's other configs are kept). Nothing is ever deleted: Kafka cannot remove partitions, so a topic
//! with more than declared keeps them and the log says so; a topic left out of the block keeps its data, because the
//! module offers no prune.

use std::collections::BTreeMap;

use doze_module::caps::{Converge, Health, Objects};
use doze_module::proto::msg::instance::{Object, Toolchain};
use doze_module::{Cx, Result, help};

use super::client::Client;
use super::{Kafka, KafkaSpec, Topic};

/// Log levels of the module protocol.
const WARN: u8 = 1;
const INFO: u8 = 2;

/// What converge did to one topic, in words for the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Change {
    Created {
        partitions: i32,
    },
    Raised {
        from: usize,
        to: i32,
    },
    /// More partitions than declared: left as they are.
    Kept {
        has: usize,
        declared: i32,
    },
    Configs(Vec<(String, String)>),
}

impl Change {
    fn level(&self) -> u8 {
        match self {
            Change::Kept { .. } => WARN,
            _ => INFO,
        }
    }

    fn say(&self, topic: &str) -> String {
        match self {
            Change::Created { partitions } => {
                format!("topic {topic}: created with {partitions} partition(s)")
            }
            Change::Raised { from, to } => {
                format!("topic {topic}: partitions raised from {from} to {to}")
            }
            Change::Kept { has, declared } => format!(
                "topic {topic}: has {has} partitions and declares {declared}; Kafka cannot remove partitions, so it keeps {has}"
            ),
            Change::Configs(set) => {
                let set: Vec<String> = set.iter().map(|(k, v)| format!("{k}={v}")).collect();
                format!("topic {topic}: config set: {}", set.join(", "))
            }
        }
    }
}

/// Brings one topic to what is declared; what was changed.
pub(crate) async fn topic(c: &mut Client, t: &Topic, has: Option<usize>) -> Result<Vec<Change>> {
    let want = t.configs();
    let has = match has {
        Some(n) => n,
        None if c.create_topic(&t.name, t.partitions, &want).await? => {
            return Ok(vec![Change::Created {
                partitions: t.partitions,
            }]);
        }
        // Made by someone else since the list was read (a client's auto-create): converge it as it is now.
        None => c
            .topics()
            .await?
            .get(&t.name)
            .map_or(0, |i| i.partitions.len()),
    };
    let mut changes = Vec::new();
    let declared = usize::try_from(t.partitions).unwrap_or(0);
    if declared > has {
        c.add_partitions(&t.name, t.partitions).await?;
        changes.push(Change::Raised {
            from: has,
            to: t.partitions,
        });
    } else if declared < has {
        changes.push(Change::Kept {
            has,
            declared: t.partitions,
        });
    }
    if !want.is_empty() {
        let mut own = c.topic_configs(&t.name).await?;
        let set: Vec<(String, String)> = want
            .iter()
            .filter(|(k, v)| own.get(*k) != Some(*v))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        if !set.is_empty() {
            own.extend(set.iter().cloned());
            c.set_topic_configs(&t.name, &own).await?;
            changes.push(Change::Configs(set));
        }
    }
    Ok(changes)
}

impl Converge for Kafka {
    async fn converge(&self, cx: &Cx<KafkaSpec>, _tc: &Toolchain) -> Result<()> {
        if cx.spec.topic.is_empty() {
            return Ok(());
        }
        let mut c = Client::connect(&cx.instance).await?;
        let have = c.topics().await?;
        for t in &cx.spec.topic {
            let has = have.get(&t.name).map(|i| i.partitions.len());
            for change in topic(&mut c, t, has).await? {
                cx.log(change.level(), change.say(&t.name));
            }
        }
        Ok(())
    }
}

impl Objects for Kafka {
    fn objects(&self, cx: &Cx<KafkaSpec>) -> Result<Vec<Object>> {
        Ok(cx
            .spec
            .topic
            .iter()
            .map(|t| {
                let configs: BTreeMap<String, String> = t.configs();
                let configs: Vec<(&str, &str)> = configs
                    .iter()
                    .map(|(k, v)| (k.as_str(), v.as_str()))
                    .collect();
                Object {
                    kind: "topic".into(),
                    name: t.name.clone(),
                    hash: help::object_hash(&(t.name.as_str(), t.partitions, configs)),
                }
            })
            .collect())
    }
}

impl Health for Kafka {
    async fn health(&self, cx: &Cx<KafkaSpec>) -> Result<()> {
        Client::connect(&cx.instance).await?.ping().await
    }
}
