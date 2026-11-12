//! Log streams: create, delete, describe.

use aws_wire::ApiError;
use serde_json::{Value, json};

use crate::core::Core;
use crate::groups::paged;
use crate::model::{Stream, stream_key};
use crate::req::{Put, Req, invalid, no_group, no_stream};

impl Core {
    pub(crate) fn create_stream(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let group = self.group(r)?.name.clone();
        let name = r.s("logStreamName").to_owned();
        let key = stream_key(&group, &name);
        if self.streams.get(&key).is_some() {
            // Recorded on AWS (2026-11-09), with no full stop.
            return Err(ApiError::modeled(
                "ResourceAlreadyExistsException",
                "The specified log stream already exists",
            ));
        }
        let no = self
            .streams
            .iter()
            .map(|(_, s)| s.no)
            .max()
            .map_or(1, |n| n + 1);
        let created_ms = self.now_ms();
        self.streams.insert(
            key,
            Stream {
                no,
                group,
                name,
                created_ms,
                first_ms: None,
                last_ms: None,
                last_ingested_ms: None,
                written: 0,
                bytes: 0,
            },
        );
        Ok(json!({}))
    }

    pub(crate) fn delete_stream(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let group = self.group(r)?.name.clone();
        let key = stream_key(&group, r.s("logStreamName"));
        let stream = self.streams.remove(&key).ok_or_else(no_stream)?;
        self.events.delete_range(
            &format!("{:08x}", stream.no),
            &format!("{:08x}", stream.no + 1),
        );
        Ok(json!({}))
    }

    pub(crate) fn describe_streams(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let group = r.group_name().unwrap_or_default();
        let g = self.groups.get(&group).ok_or_else(no_group)?;
        let by_time = r.opt("orderBy") == Some("LastEventTime");
        let prefix = r.opt("logStreamNamePrefix");
        if by_time && prefix.is_some() {
            // (memory)
            return Err(invalid(
                "Cannot order by LastEventTime with a logStreamNamePrefix.",
            ));
        }
        let own = stream_key(&g.name, "");
        let mut all: Vec<&Stream> = self
            .streams
            .with_prefix(&own)
            .map(|(_, s)| s)
            .filter(|s| prefix.is_none_or(|p| s.name.starts_with(p)))
            .collect();
        if by_time {
            all.sort_by_key(|s| (s.last_ms.unwrap_or(0), s.name.clone()));
        }
        if r.flag("descending") == Some(true) {
            all.reverse();
        }
        let (page, token) = paged(&all, r, 50, |s| s.name.as_str());
        let shown: Vec<Value> = page
            .iter()
            .map(|s| stream_json(s, g, r.region(), r.account()))
            .collect();
        let mut out = json!({ "logStreams": shown });
        if let Some(t) = token {
            out.put("nextToken", json!(t));
        }
        Ok(out)
    }
}

fn stream_json(s: &Stream, g: &crate::model::Group, region: &str, account: &str) -> Value {
    let mut v = json!({
        "logStreamName": s.name,
        "creationTime": s.created_ms,
        "arn": format!("{}:log-stream:{}", g.arn(region, account), s.name),
    });
    if let (Some(first), Some(last), Some(ingested)) = (s.first_ms, s.last_ms, s.last_ingested_ms) {
        v.put("firstEventTimestamp", json!(first));
        v.put("lastEventTimestamp", json!(last));
        v.put("lastIngestionTime", json!(ingested));
        v.put("uploadSequenceToken", json!(crate::events::token_of(s)));
    }
    v
}
