//! Stream consumers (enhanced fan-out): registered and listed, and nothing more. `SubscribeToShard` needs HTTP/2 and is refused by
//! name, so a registered consumer cannot receive; the registration is here because a template that declares a consumer must apply.

use aws_wire::ApiError;
use serde_json::{Value, json};

use crate::core::Core;
use crate::model::{Consumer, Stream};
use crate::req::{Put, Req, invalid, name_of_arn, secs};

/// Consumers one stream may have (AWS: 20).
const MAX_CONSUMERS: usize = 20;

fn consumer_arn(stream_arn: &str, c: &Consumer) -> String {
    format!("{stream_arn}/consumer/{}:{}", c.name, c.created_ms / 1000)
}

fn shown(stream_arn: &str, c: &Consumer) -> Value {
    json!({
        "ConsumerName": c.name,
        "ConsumerARN": consumer_arn(stream_arn, c),
        "ConsumerStatus": "ACTIVE",
        "ConsumerCreationTimestamp": secs(c.created_ms),
    })
}

/// The consumer a request names, by `ConsumerName` on its stream or by `ConsumerARN`.
fn find<'a>(s: &'a Stream, r: &Req<'_>) -> Option<&'a Consumer> {
    let arn = r.opt("ConsumerARN");
    let name = match arn {
        Some(a) => a.split_once("/consumer/")?.1.rsplit_once(':')?.0,
        None => r.opt("ConsumerName")?,
    };
    s.consumers.iter().find(|c| c.name == name)
}

impl Core {
    /// The stream a consumer call names: `StreamARN`, or the stream inside a `ConsumerARN`.
    fn consumer_stream(&self, r: &Req<'_>) -> Result<&Stream, ApiError> {
        let name = r
            .opt("StreamARN")
            .and_then(name_of_arn)
            .or_else(|| r.opt("ConsumerARN").and_then(name_of_arn))
            .unwrap_or_default();
        self.streams
            .get(&name)
            .ok_or_else(|| crate::core::not_found(&name, r.account()))
    }

    pub(crate) fn register_consumer(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let now = self.now_ms();
        let arn = r.s("StreamARN").to_owned();
        let name = r.s("ConsumerName").to_owned();
        let s = self.stream_mut(r)?;
        if s.consumers.iter().any(|c| c.name == name) {
            // (memory)
            return Err(ApiError::modeled(
                "ResourceInUseException",
                format!(
                    "Consumer {name} under stream {} and account {} already exists.",
                    s.name,
                    arn.split(':').nth(4).unwrap_or_default()
                ),
            ));
        }
        if s.consumers.len() >= MAX_CONSUMERS {
            // (memory)
            return Err(ApiError::modeled(
                "LimitExceededException",
                format!("A stream can have at most {MAX_CONSUMERS} consumers."),
            ));
        }
        let c = Consumer {
            name,
            created_ms: now,
            tags: r.map("Tags").into_iter().collect(),
        };
        let out = shown(&arn, &c);
        s.consumers.push(c);
        Ok(json!({"Consumer": out}))
    }

    pub(crate) fn deregister_consumer(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let found = find(self.consumer_stream(r)?, r).map(|c| c.name.clone());
        let name = found.ok_or_else(not_registered)?;
        let stream = self.consumer_stream(r)?.name.clone();
        if let Some(s) = self.streams.get_mut(&stream) {
            s.consumers.retain(|c| c.name != name);
        }
        Ok(json!({}))
    }

    pub(crate) fn describe_consumer(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let s = self.consumer_stream(r)?;
        let arn = s.arn(r.region(), r.account());
        let c = find(s, r).ok_or_else(not_registered)?;
        let mut d = shown(&arn, c);
        d.put("StreamARN", json!(arn));
        Ok(json!({"ConsumerDescription": d}))
    }

    pub(crate) fn list_consumers(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let s = self.consumer_stream(r)?;
        let arn = s.arn(r.region(), r.account());
        let limit = r
            .int("MaxResults")
            .map_or(100, |n| usize::try_from(n).unwrap_or(100));
        let after = r.opt("NextToken").map(str::to_owned);
        if after.as_deref().is_some_and(str::is_empty) {
            return Err(invalid("Invalid NextToken."));
        }
        let rest: Vec<_> = s
            .consumers
            .iter()
            .filter(|c| after.as_deref().is_none_or(|a| c.name.as_str() > a))
            .collect();
        let mut out = json!({
            "Consumers": rest.iter().take(limit).map(|c| shown(&arn, c)).collect::<Vec<_>>(),
        });
        if let (true, Some(last)) = (rest.len() > limit, rest.get(limit.saturating_sub(1))) {
            out.put("NextToken", json!(last.name));
        }
        Ok(out)
    }
}

/// (memory)
fn not_registered() -> ApiError {
    ApiError::modeled(
        "ResourceNotFoundException",
        "The consumer is not registered with the stream.",
    )
}
