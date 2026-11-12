//! The configurations that come many to a bucket, each under an id: analytics, inventory, metrics and intelligent tiering. Nothing
//! here acts on them (no report is produced, nothing is tiered); a template or Terraform run puts them and reads them back, and
//! they are kept as sent and returned as sent, which is all that run asks.

use aws_model_s3::Operation;
use aws_wire::ApiError;
use serde_json::{Value, json};

use crate::core::Core;
use crate::errors::malformed_xml;
use crate::req::{Answer, Out, Req};

struct Family {
    name: &'static str,
    put: Operation,
    get: Operation,
    delete: Operation,
    list: Operation,
    /// The configuration's member name in the request and in the answer.
    member: &'static str,
    /// The list's member name in a listing.
    list_member: &'static str,
}

use Operation as O;

const FAMILIES: [Family; 4] = [
    Family {
        name: "analytics",
        put: O::PutBucketAnalyticsConfiguration,
        get: O::GetBucketAnalyticsConfiguration,
        delete: O::DeleteBucketAnalyticsConfiguration,
        list: O::ListBucketAnalyticsConfigurations,
        member: "AnalyticsConfiguration",
        list_member: "AnalyticsConfigurationList",
    },
    Family {
        name: "inventory",
        put: O::PutBucketInventoryConfiguration,
        get: O::GetBucketInventoryConfiguration,
        delete: O::DeleteBucketInventoryConfiguration,
        list: O::ListBucketInventoryConfigurations,
        member: "InventoryConfiguration",
        list_member: "InventoryConfigurationList",
    },
    Family {
        name: "metrics",
        put: O::PutBucketMetricsConfiguration,
        get: O::GetBucketMetricsConfiguration,
        delete: O::DeleteBucketMetricsConfiguration,
        list: O::ListBucketMetricsConfigurations,
        member: "MetricsConfiguration",
        list_member: "MetricsConfigurationList",
    },
    Family {
        name: "intelligent-tiering",
        put: O::PutBucketIntelligentTieringConfiguration,
        get: O::GetBucketIntelligentTieringConfiguration,
        delete: O::DeleteBucketIntelligentTieringConfiguration,
        list: O::ListBucketIntelligentTieringConfigurations,
        member: "IntelligentTieringConfiguration",
        list_member: "IntelligentTieringConfigurationList",
    },
];

fn family(op: Operation) -> Option<&'static Family> {
    FAMILIES
        .iter()
        .find(|f| [f.put, f.get, f.delete, f.list].contains(&op))
}

/// Whether `op` belongs to one of the families.
pub(crate) fn is_id_config(op: Operation) -> bool {
    family(op).is_some()
}

fn no_such(id: &str) -> ApiError {
    ApiError::new(
        404,
        "NoSuchConfiguration",
        "The specified configuration does not exist.",
    )
    .with("Id", id)
}

impl Core {
    pub(crate) fn id_config(&mut self, req: &Req<'_>, op: Operation) -> Answer {
        let Some(f) = family(op) else {
            return Err(ApiError::new(
                501,
                "NotImplemented",
                "This operation is not built in doze-aws.",
            ));
        };
        let bucket = req.bucket();
        self.require(bucket)?;
        let id = req.s("Id").unwrap_or_default();
        let key = format!("{}:{id}", f.name);
        if op == f.put {
            let body = req.get(f.member).ok_or_else(malformed_xml)?;
            if body
                .get("Id")
                .and_then(Value::as_str)
                .is_some_and(|b| b != id)
            {
                return Err(ApiError::new(
                    400,
                    "IdMismatch",
                    "Id in the request body does not match the Id in the query string",
                ));
            }
            if let Some(b) = self.buckets.get_mut(bucket) {
                b.configs.insert(key, body.to_string());
            }
            return Out::empty();
        }
        if op == f.get {
            let found = self
                .require(bucket)?
                .configs
                .get(&key)
                .ok_or_else(|| no_such(id))?;
            return Out::json(
                json!({ f.member: serde_json::from_str::<Value>(found).unwrap_or(Value::Null) }),
            );
        }
        if op == f.delete {
            let removed = self
                .buckets
                .get_mut(bucket)
                .and_then(|b| b.configs.remove(&key));
            return removed
                .map(|_| ())
                .ok_or_else(|| no_such(id))
                .and_then(|()| Out::empty());
        }
        let prefix = format!("{}:", f.name);
        let all: Vec<Value> = self
            .require(bucket)?
            .configs
            .iter()
            .filter(|(k, _)| k.starts_with(&prefix))
            .filter_map(|(_, v)| serde_json::from_str(v).ok())
            .collect();
        Out::json(json!({ "IsTruncated": false, f.list_member: all }))
    }
}
