//! ListConfigResources (API 74, KIP-1142; v0 is Kafka 3.x's ListClientMetricsResources, KIP-714): the resources that can
//! have configs, of the types asked for. What `kafka-configs.sh --describe` without an entity name and the admin clients'
//! `listConfigResources` (and `listClientMetricsResources`) send.
//!
//! Kafka 4.4.0's `KafkaApis.handleListConfigResources`: v0 knows CLIENT_METRICS alone and v1 TOPIC, BROKER, BROKER_LOGGER,
//! CLIENT_METRICS and GROUP; no type named means every one the version knows, and a type it does not know is
//! UNSUPPORTED_VERSION with nothing listed. The resources come by type in Kafka's order: the groups with a config set, the
//! client-metrics subscriptions (none here: this broker has no CLIENT_METRICS resource, SEC-D100), the broker as a logger, the
//! broker, then every topic.

use kafka_auth::authz::Operation;
use kafka_storage::fs::Fs;
use kafka_wire::ResponseError;
use kafka_wire::StrBytes;
use kafka_wire::messages::list_config_resources_response::ConfigResource;
use kafka_wire::messages::{ListConfigResourcesRequest, ListConfigResourcesResponse};

use crate::{Broker, Caller};

/// The config resource types (`ConfigResource.Type`).
const TOPIC: i8 = 2;
const BROKER: i8 = 4;
const BROKER_LOGGER: i8 = 8;
const CLIENT_METRICS: i8 = 16;
const GROUP: i8 = 32;

impl<F: Fs> Broker<F> {
    /// The resources of the types asked for. The caller needs DESCRIBE_CONFIGS on the cluster; without it the answer is
    /// CLUSTER_AUTHORIZATION_FAILED and nothing listed.
    pub(crate) fn list_config_resources(
        &self,
        caller: &Caller,
        req: &ListConfigResourcesRequest,
        version: i16,
    ) -> ListConfigResourcesResponse {
        let mut resp = ListConfigResourcesResponse::default();
        if !self.cluster_allows(caller, Operation::DescribeConfigs) {
            resp.error_code = ResponseError::ClusterAuthorizationFailed.code();
            return resp;
        }
        let supported: &[i8] = if version == 0 {
            &[CLIENT_METRICS]
        } else {
            &[TOPIC, BROKER, BROKER_LOGGER, CLIENT_METRICS, GROUP]
        };
        let asked: &[i8] = if req.resource_types.is_empty() {
            supported
        } else {
            &req.resource_types
        };
        if asked.iter().any(|t| !supported.contains(t)) {
            resp.error_code = ResponseError::UnsupportedVersion.code();
            return resp;
        }
        let node = self.config.node_id.to_string();
        let mut add = |kind: i8, name: String| {
            let mut r = ConfigResource::default();
            r.resource_name = StrBytes::from_string(name);
            r.resource_type = kind;
            resp.config_resources.push(r);
        };
        if asked.contains(&GROUP) {
            for group in self.group_configs.groups() {
                add(GROUP, group);
            }
        }
        if asked.contains(&BROKER_LOGGER) {
            add(BROKER_LOGGER, node.clone());
        }
        if asked.contains(&BROKER) {
            add(BROKER, node);
        }
        if asked.contains(&TOPIC) {
            for topic in self.meta.topics() {
                add(TOPIC, topic.name.to_string());
            }
        }
        resp
    }
}
