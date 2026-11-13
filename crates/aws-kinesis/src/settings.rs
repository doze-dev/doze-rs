//! Stream settings: retention, encryption, enhanced monitoring, record size, warm throughput, the resource policy. Encryption and
//! monitoring are stored and returned; nothing here is applied (a record is not encrypted, no shard-level metric is published).

use aws_wire::ApiError;
use serde_json::{Value, json};

use crate::core::{Core, not_found};
use crate::req::{Req, invalid, name_of_arn};

/// The shortest and longest retention a stream may have.
const MIN_HOURS: i64 = 24;
const MAX_HOURS: i64 = 8760;
/// The shard-level metrics `ALL` stands for.
const METRICS: [&str; 7] = [
    "IncomingBytes",
    "IncomingRecords",
    "OutgoingBytes",
    "OutgoingRecords",
    "WriteProvisionedThroughputExceeded",
    "ReadProvisionedThroughputExceeded",
    "IteratorAgeMilliseconds",
];

impl Core {
    pub(crate) fn change_retention(
        &mut self,
        r: &Req<'_>,
        increase: bool,
    ) -> Result<Value, ApiError> {
        let hours = r.int("RetentionPeriodHours").unwrap_or(0);
        let s = self.stream_mut(r)?;
        // The minimum is recorded on AWS (2026-11-09); the maximum is worded alike (memory).
        if hours < MIN_HOURS {
            return Err(invalid(format!(
                "Minimum allowed retention period is {MIN_HOURS} hours. Requested retention period ({hours} hours) is too short."
            )));
        }
        if hours > MAX_HOURS {
            return Err(invalid(format!(
                "Maximum allowed retention period is {MAX_HOURS} hours. Requested retention period ({hours} hours) is too long."
            )));
        }
        // Asking for what the stream already has is accepted (recorded on AWS); the two refusals below are worded from memory.
        if increase && hours < s.retention_hours {
            return Err(invalid(format!(
                "Requested retention period ({hours} hours) for stream {} under account {} is not longer than existing retention period ({} hours).",
                s.name,
                r.account(),
                s.retention_hours
            )));
        }
        if !increase && hours > s.retention_hours {
            return Err(invalid(format!(
                "Requested retention period ({hours} hours) for stream {} under account {} is not shorter than existing retention period ({} hours).",
                s.name,
                r.account(),
                s.retention_hours
            )));
        }
        s.retention_hours = hours;
        Ok(json!({}))
    }

    pub(crate) fn start_encryption(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let key = r.s("KeyId").to_owned();
        self.stream_mut(r)?.kms_key = Some(key);
        Ok(json!({}))
    }

    pub(crate) fn stop_encryption(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        self.stream_mut(r)?.kms_key = None;
        Ok(json!({}))
    }

    pub(crate) fn monitoring(&mut self, r: &Req<'_>, enable: bool) -> Result<Value, ApiError> {
        let asked = r.strings("ShardLevelMetrics");
        let named: Vec<String> = if asked.iter().any(|m| m == "ALL") {
            METRICS.iter().map(|m| (*m).to_owned()).collect()
        } else {
            asked
        };
        let region = r.region().to_owned();
        let account = r.account().to_owned();
        let s = self.stream_mut(r)?;
        let before: Vec<String> = s.metrics.iter().cloned().collect();
        for m in named {
            if enable {
                s.metrics.insert(m);
            } else {
                s.metrics.remove(&m);
            }
        }
        Ok(json!({
            "StreamName": s.name,
            "StreamARN": s.arn(&region, &account),
            "CurrentShardLevelMetrics": before,
            "DesiredShardLevelMetrics": s.metrics.iter().collect::<Vec<_>>(),
        }))
    }

    pub(crate) fn update_record_size(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let kib = r.int("MaxRecordSizeInKiB").unwrap_or(1024);
        self.stream_mut(r)?.max_record_kib = kib;
        Ok(json!({}))
    }

    pub(crate) fn update_warm_throughput(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let mibps = r.int("WarmThroughputMiBps");
        let region = r.region().to_owned();
        let account = r.account().to_owned();
        let s = self.stream_mut(r)?;
        s.warm_mibps = mibps;
        Ok(json!({"StreamName": s.name, "StreamARN": s.arn(&region, &account)}))
    }

    /// The stream a resource policy call names, by `ResourceARN`.
    fn policy_stream(&mut self, r: &Req<'_>) -> Result<&mut crate::model::Stream, ApiError> {
        let name = name_of_arn(r.s("ResourceARN")).unwrap_or_default();
        let account = r.account().to_owned();
        self.streams
            .get_mut(&name)
            .ok_or_else(|| not_found(&name, &account))
    }

    pub(crate) fn put_policy(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let policy = r.s("Policy").to_owned();
        self.policy_stream(r)?.policy = Some(policy);
        Ok(json!({}))
    }

    pub(crate) fn get_policy(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let arn = r.s("ResourceARN").to_owned();
        match self.policy_stream(r)?.policy.clone() {
            Some(p) => Ok(json!({"Policy": p})),
            // The Go edition's finding G7: AWS answers a stream with no policy as not found (the words are memory).
            None => Err(ApiError::modeled(
                "ResourceNotFoundException",
                format!("No resource policy is attached to {arn}."),
            )),
        }
    }

    pub(crate) fn delete_policy(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        self.policy_stream(r)?.policy = None;
        Ok(json!({}))
    }
}
