//! DescribeConfigs: the configs of a topic or of the broker, each with where its value comes from (source:
//! `ConfigHelper.describeConfigs`).

use kafka_storage::fs::Fs;
use kafka_wire::ResponseError as E;
use kafka_wire::StrBytes;
use kafka_wire::api::Response;
use kafka_wire::ids::TopicName;
use kafka_wire::messages::describe_configs_request::DescribeConfigsResource;
use kafka_wire::messages::describe_configs_response::{
    DescribeConfigsResourceResult as Config, DescribeConfigsResult,
    DescribeConfigsSynonym as Synonym,
};
use kafka_wire::messages::{DescribeConfigsRequest, DescribeConfigsResponse};

use crate::configs::view::{self, View};
use crate::configs::{Live, synonyms};
use crate::handlers::alter_configs::{BROKER, TOPIC, unsupported};
use crate::handlers::create_topics::name_message;
use crate::topic_config::{self, Entry, Source};
use crate::{Broker, Outcome};

/// Why a resource has no configs to give.
struct Refused(E, Option<String>);

fn text(s: &str) -> StrBytes {
    StrBytes::from_string(s.to_owned())
}

fn synonym(name: &str, value: &str, source: i8) -> Synonym {
    let mut s = Synonym::default();
    s.name = text(name);
    s.value = Some(text(value));
    s.source = source;
    s
}

impl<F: Fs> Broker<F> {
    pub(crate) fn describe_configs(&self, req: &DescribeConfigsRequest) -> Outcome {
        let mut resp = DescribeConfigsResponse::default();
        for r in &req.resources {
            let mut out = DescribeConfigsResult::default();
            out.resource_type = r.resource_type;
            out.resource_name = r.resource_name.clone();
            match self.configs_of(r, req) {
                Ok(configs) => out.configs = configs,
                Err(Refused(code, message)) => {
                    out.error_code = code.code();
                    out.error_message = message.as_deref().map(text);
                }
            }
            resp.results.push(out);
        }
        Outcome::Respond(Response::DescribeConfigs(resp))
    }

    fn configs_of(
        &self,
        r: &DescribeConfigsResource,
        req: &DescribeConfigsRequest,
    ) -> Result<Vec<Config>, Refused> {
        let name = r.resource_name.as_str();
        // Naming no keys, or none, is asking for all.
        let wanted = |n: &str| {
            r.configuration_keys
                .as_ref()
                .is_none_or(|keys| keys.is_empty() || keys.iter().any(|k| k.as_str() == n))
        };
        match r.resource_type {
            TOPIC => {
                let topic = TopicName::parse(name)
                    .map_err(|e| Refused(E::InvalidTopicException, Some(name_message(e, name))))?;
                let topic = self
                    .meta
                    .topic(&topic)
                    .ok_or(Refused(E::UnknownTopicOrPartition, None))?;
                let defaults = self.defaults.get();
                Ok(topic_config::effective(&defaults, &topic.config)
                    .into_iter()
                    .filter(|e| wanted(e.name))
                    .map(|e| topic_entry(&e, &topic.config, &self.defaults, req))
                    .collect())
            }
            BROKER => {
                let views = if name.is_empty() {
                    view::of_cluster(&self.defaults)
                } else {
                    let id: i32 = name.parse().map_err(|_| {
                        Refused(
                            E::InvalidRequest,
                            Some(format!("Broker id must be an integer, but it is: {name}")),
                        )
                    })?;
                    if id != self.config.node_id {
                        return Err(Refused(
                            E::InvalidRequest,
                            Some(format!(
                                "Unexpected broker id, expected {} or empty string, but received {name}",
                                self.config.node_id
                            )),
                        ));
                    }
                    view::of_broker(&self.config, &self.defaults)
                };
                Ok(views
                    .iter()
                    .filter(|v| wanted(v.name))
                    .map(|v| broker_entry(v, name.is_empty(), req))
                    .collect())
            }
            other => {
                let u = unsupported(other);
                Err(Refused(u.code, Some(u.message)))
            }
        }
    }
}

fn topic_entry(
    e: &Entry,
    own: &std::collections::BTreeMap<String, String>,
    live: &Live,
    req: &DescribeConfigsRequest,
) -> Config {
    let mut c = Config::default();
    c.name = text(e.name);
    c.value = Some(text(&e.value));
    c.config_type = e.kind;
    // Where the value comes from is the first place it was given: the topic, else the broker's places in order.
    let behind = synonyms::of_topic_key(live, e.name);
    c.config_source = if own.contains_key(e.name) {
        Source::DynamicTopic.code()
    } else {
        behind.first().map_or(e.source.code(), |s| s.2)
    };
    if req.include_synonyms {
        if own.contains_key(e.name) {
            c.synonyms
                .push(synonym(e.name, &e.value, Source::DynamicTopic.code()));
        }
        c.synonyms.extend(
            behind
                .into_iter()
                .map(|(name, value, source)| synonym(&name, &value, source)),
        );
    }
    c.documentation = None;
    if req.include_documentation {
        c.documentation = Some(text(e.doc));
    }
    c
}

fn broker_entry(v: &View, cluster: bool, req: &DescribeConfigsRequest) -> Config {
    let mut c = Config::default();
    c.name = text(v.name);
    c.value = Some(text(&v.value));
    c.read_only = v.read_only;
    c.documentation = None;
    c.config_source = v.source;
    c.config_type = v.kind;
    if req.include_synonyms {
        c.synonyms = v
            .synonyms
            .iter()
            // The configs of all brokers keep only the places that are for all brokers.
            .filter(|(source, _)| !cluster || *source == view::DYNAMIC_DEFAULT)
            .map(|(source, value)| synonym(v.name, value, *source))
            .collect();
    }
    c
}
