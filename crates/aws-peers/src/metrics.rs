//! Metrics a service makes and holds until CloudWatch needs them. Lambda counts its invocations in memory; nothing is sent while
//! nothing reads them. CloudWatch takes what is held before a read ([`Peers::take_metrics`]), and says which namespaces an alarm
//! watches ([`Peers::watch_namespaces`]): a producer sends those at once ([`Peers::watched`]), so an alarm judges them as they come.

use std::collections::HashMap;
use std::sync::{Arc, PoisonError};

use serde_json::Value;

use crate::Peers;

/// Data points a producer held, as the `PutMetricData` request its service makes of them.
#[derive(Debug, Clone)]
pub struct Made {
    /// The service that made them (it acts as itself when they are put).
    pub service: &'static str,
    /// The account they belong to.
    pub account: String,
    /// The `PutMetricData` request: `Namespace` and `MetricData`.
    pub body: Value,
}

/// A producer: hands over, and forgets, what it holds for a region.
pub type Producer = Arc<dyn Fn(&str) -> Vec<Made> + Send + Sync>;

#[derive(Default)]
pub(crate) struct Registry {
    pub(crate) producers: Vec<Producer>,
    /// The namespaces a metric alarm watches, by region.
    pub(crate) watched: HashMap<String, Vec<String>>,
}

impl Peers {
    /// Adds a producer whose data points CloudWatch takes when it reads.
    pub fn add_producer(&self, producer: Producer) {
        self.metrics
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .producers
            .push(producer);
    }

    /// What every producer holds for `region`, taken from them.
    #[must_use]
    pub fn take_metrics(&self, region: &str) -> Vec<Made> {
        let producers: Vec<Producer> = self
            .metrics
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .producers
            .clone();
        producers.iter().flat_map(|p| p(region)).collect()
    }

    /// Says which namespaces the metric alarms of `region` watch (all of them, replacing what was said before).
    pub fn watch_namespaces(&self, region: &str, namespaces: Vec<String>) {
        let mut m = self.metrics.write().unwrap_or_else(PoisonError::into_inner);
        if namespaces.is_empty() {
            m.watched.remove(region);
        } else {
            m.watched.insert(region.to_owned(), namespaces);
        }
    }

    /// Whether a metric alarm in `region` watches `namespace`: its producer then sends data points as it makes them.
    #[must_use]
    pub fn watched(&self, region: &str, namespace: &str) -> bool {
        self.metrics
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .watched
            .get(region)
            .is_some_and(|n| n.iter().any(|x| x == namespace))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::indexing_slicing)]
    use serde_json::json;

    use super::*;

    #[test]
    fn producers_are_drained_by_region_and_watching_is_by_region() {
        let peers = Peers::new();
        peers.add_producer(Arc::new(|region: &str| {
            vec![Made {
                service: "lambda",
                account: "1".into(),
                body: json!({"Namespace": "AWS/Lambda", "region": region}),
            }]
        }));
        let made = peers.take_metrics("eu-west-1");
        assert_eq!(made.len(), 1);
        assert_eq!(made[0].body["region"], "eu-west-1");
        assert!(!peers.watched("us-east-1", "AWS/Lambda"));
        peers.watch_namespaces("us-east-1", vec!["AWS/Lambda".into()]);
        assert!(peers.watched("us-east-1", "AWS/Lambda"));
        assert!(!peers.watched("eu-west-1", "AWS/Lambda"));
        peers.watch_namespaces("us-east-1", Vec::new());
        assert!(!peers.watched("us-east-1", "AWS/Lambda"));
    }
}
