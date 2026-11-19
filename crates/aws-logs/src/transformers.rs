//! The transformer of a log group (`PutTransformer`, `GetTransformer`, `DeleteTransformer`, `TestTransformer`), and its use at
//! ingestion: each event `PutLogEvents` takes is run through the group's transformer and the result kept beside the original. Logs
//! Insights reads the transformed fields; metric and subscription filters with `applyOnTransformedLogs` match and send the
//! transformed event; `GetLogEvents` and `FilterLogEvents` answer the original (memory). The processors are `transform`.

use aws_wire::ApiError;
use serde_json::{Value, json};

use crate::core::Core;
use crate::events::Incoming;
use crate::model::{Doc, doc_key, group_arn};
use crate::req::{Req, name_of_identifier, no_group};
use crate::transform::{Meta, Transformer};

pub(crate) const TRANSFORMER: &str = "transformer";

/// (memory)
fn no_transformer() -> ApiError {
    ApiError::modeled(
        "ResourceNotFoundException",
        "The specified log group does not have a transformer.",
    )
}

impl Core {
    /// The name of the group a `logGroupIdentifier` names, which must exist.
    pub(crate) fn identified(&self, r: &Req<'_>) -> Result<String, ApiError> {
        let name = name_of_identifier(r.s("logGroupIdentifier"));
        self.groups
            .get(&name)
            .map(|g| g.name.clone())
            .ok_or_else(no_group)
    }

    pub(crate) fn put_transformer(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = self.identified(r)?;
        if self
            .groups
            .get(&name)
            .is_some_and(|g| g.class != "STANDARD")
        {
            // The model: "You can create transformers only for the log groups in the Standard log class." Wording (memory).
            return Err(ApiError::modeled(
                "InvalidOperationException",
                "Transformers can be created only for log groups in the Standard log class.",
            ));
        }
        let config = r.v.get("transformerConfig").cloned().unwrap_or(Value::Null);
        Transformer::parse(&config)?;
        let now = self.now_ms();
        let created = self
            .stored_transformer(&name)
            .and_then(|t| t.get("creationTime").and_then(Value::as_i64))
            .unwrap_or(now);
        let doc =
            json!({"creationTime": created, "lastModifiedTime": now, "transformerConfig": config});
        self.docs.insert(
            doc_key(TRANSFORMER, &name),
            Doc {
                json: doc.to_string(),
                updated_ms: now,
            },
        );
        Ok(json!({}))
    }

    fn stored_transformer(&self, group: &str) -> Option<Value> {
        self.docs
            .get(&doc_key(TRANSFORMER, group))
            .and_then(|d| serde_json::from_str(&d.json).ok())
    }

    pub(crate) fn get_transformer(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = self.identified(r)?;
        let mut t = self.stored_transformer(&name).ok_or_else(no_transformer)?;
        crate::req::Put::put(
            &mut t,
            "logGroupIdentifier",
            json!(group_arn(r.region(), r.account(), &name)),
        );
        Ok(t)
    }

    pub(crate) fn delete_transformer(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = self.identified(r)?;
        self.docs
            .remove(&doc_key(TRANSFORMER, &name))
            .ok_or_else(no_transformer)?;
        Ok(json!({}))
    }

    /// `TestTransformer` needs no group. An event the transformer makes nothing of is shown as it came. (memory)
    pub(crate) fn test_transformer(r: &Req<'_>) -> Result<Value, ApiError> {
        let config = r.v.get("transformerConfig").cloned().unwrap_or(Value::Null);
        let t = Transformer::parse(&config)?;
        let meta = Meta {
            group: "",
            stream: "",
            account: r.account(),
            region: r.region(),
        };
        let out: Vec<Value> = r
            .strings("logEventMessages")
            .iter()
            .enumerate()
            .map(|(i, m)| {
                json!({
                    "eventNumber": i + 1,
                    "eventMessage": m,
                    "transformedEventMessage": t.apply(m, &meta).unwrap_or_else(|| m.clone()),
                })
            })
            .collect();
        Ok(json!({"transformedLogs": out}))
    }

    /// What the group's transformer makes of each event (`None` for each when it has none).
    pub(crate) fn transformed(
        &self,
        group: &str,
        r: &Req<'_>,
        events: &[Incoming],
    ) -> Vec<Option<String>> {
        let t = self
            .stored_transformer(group)
            .and_then(|t| t.get("transformerConfig").cloned())
            .and_then(|c| Transformer::parse(&c).ok());
        let Some(t) = t else {
            return vec![None; events.len()];
        };
        let meta = Meta {
            group,
            stream: r.s("logStreamName"),
            account: r.account(),
            region: r.region(),
        };
        events.iter().map(|e| t.apply(&e.message, &meta)).collect()
    }
}
