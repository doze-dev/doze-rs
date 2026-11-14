//! The Config page of a topic: DescribeConfigs with the places each value could come from and Kafka's text, read into rows.

use kafka_wire::api::Response;
use kafka_wire::messages::describe_configs_response::DescribeConfigsResourceResult;

use super::configs::cleanup_of;
use crate::backend::{Backend, requests::alter_configs};
use crate::model::{
    ConfigAsk, ConfigHead, ConfigPace, ConfigPage, ConfigRow, ConfigSource, ConfigSynonym,
};

/// What was asked for that is not there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigMissing {
    /// No such topic (or the console may not describe it).
    Topic,
    /// The topic has no such config.
    Key,
}

fn row_of(c: &DescribeConfigsResourceResult, allowed: &str) -> ConfigRow {
    ConfigRow {
        name: c.name.to_string(),
        value: c
            .value
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default(),
        source: ConfigSource::from_wire(c.config_source),
        kind: c.config_type,
        doc: c
            .documentation
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default(),
        allowed: allowed.to_owned(),
        synonyms: c
            .synonyms
            .iter()
            .map(|s| ConfigSynonym {
                name: s.name.to_string(),
                value: s
                    .value
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_default(),
                source: ConfigSource::from_wire(s.source),
            })
            .collect(),
    }
}

/// Every config of the topic, in the order Kafka lists them; `None` if there is no such topic or the console may not
/// describe it.
pub(super) async fn rows(b: &Backend, topic: &str) -> Option<Vec<ConfigRow>> {
    let Ok(Response::DescribeConfigs(r)) = b.call(alter_configs::describe_all(topic)).await else {
        return None;
    };
    let res = r.results.first().filter(|x| x.error_code == 0)?;
    let rules = b.topic_config_rules(topic);
    Some(
        res.configs
            .iter()
            .map(|c| {
                let allowed = rules
                    .iter()
                    .find(|x| x.name == c.name.as_str())
                    .map_or("", |x| x.allowed.as_str());
                row_of(c, allowed)
            })
            .collect(),
    )
}

/// How often the broker looks at logs, if the console may know.
#[must_use]
pub(super) fn pace(b: &Backend) -> Option<ConfigPace> {
    b.pace().map(|p| ConfigPace {
        cleaner_secs: p.cleaner_every.as_secs(),
        cleaner_on: p.cleaner_on,
        retention_secs: p.retention_every.as_secs(),
    })
}

/// The title row for the rows read.
#[must_use]
pub(super) fn head(topic: &str, rows: &[ConfigRow]) -> ConfigHead {
    let policy = rows
        .iter()
        .find(|r| r.name == "cleanup.policy")
        .map_or("delete", |r| r.value.as_str());
    ConfigHead {
        name: topic.to_owned(),
        cleanup: cleanup_of(policy),
    }
}

/// The page; `None` if there is no such topic.
pub async fn page(b: &Backend, topic: &str, ask: ConfigAsk) -> Option<ConfigPage> {
    let rows = rows(b, topic).await?;
    Some(ConfigPage {
        head: head(topic, &rows),
        pace: pace(b),
        rows,
        ask,
    })
}
