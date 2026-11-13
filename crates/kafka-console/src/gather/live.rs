//! What the broker says of a topic NOW, for the rules of a refusal drawer: the cleanup policy and the size limit in force.

use kafka_wire::api::Response;

use crate::backend::{Backend, requests};
use crate::live_words::Limits as RequestLimits;

/// The config source of a value set on the topic itself.
const DYNAMIC_TOPIC: i8 = 1;

/// The limits of `topic` as the broker says them now. A topic that is gone, or that the console may not describe, has none.
pub async fn request_limits(b: &Backend, topic: &str) -> RequestLimits {
    let Ok(Response::DescribeConfigs(r)) = b
        .call(requests::describe_topic_configs(&[topic.to_owned()]))
        .await
    else {
        return RequestLimits::default();
    };
    let mut out = RequestLimits::default();
    for res in r.results.iter().filter(|r| r.error_code == 0) {
        for cfg in &res.configs {
            let value = cfg.value.as_ref().map(ToString::to_string);
            match (cfg.name.as_str(), value) {
                ("cleanup.policy", Some(v)) => out.cleanup = Some(v),
                ("max.message.bytes", Some(v)) => {
                    out.max_message_bytes = v
                        .parse::<u64>()
                        .ok()
                        .map(|n| (n, cfg.config_source == DYNAMIC_TOPIC));
                }
                _ => {}
            }
        }
    }
    out
}
