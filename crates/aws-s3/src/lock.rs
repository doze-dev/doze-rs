//! Object Lock: retention (GOVERNANCE and COMPLIANCE) and legal holds on object versions, the bucket's default retention, and what
//! they forbid. A locked version cannot be removed by a delete that names it (or by lifecycle), however the caller is allowed; a
//! plain delete in a versioned bucket only adds a delete marker and so is not stopped. Also `GetBucketPolicyStatus`.
//!
//! Wording marked `(memory)` was not in a recording.

use aws_wire::ApiError;
use serde_json::{Value, json};

use crate::core::Core;
use crate::errors::{invalid_argument, invalid_request, malformed_xml, no_config};
use crate::model::{Lock, Mode, Retention, Version};
use crate::req::{Answer, Out, Req};
use crate::timeutil::parse_http_date;

const DAY_MS: i64 = 86_400_000;

/// The refusal of a delete or an overwrite of a locked version (also what `DeleteObjects` reports for the key).
pub(crate) fn locked() -> ApiError {
    // (memory) AWS's wording.
    ApiError::new(
        403,
        "AccessDenied",
        "Access Denied because object protected by object lock.",
    )
}

/// Retention or a legal hold on a bucket that was never given Object Lock.
fn missing_configuration() -> ApiError {
    // (memory) AWS's wording.
    invalid_request("Bucket is missing Object Lock Configuration")
}

/// An object that was never given retention (or a legal hold).
fn no_object_lock() -> ApiError {
    // (memory) AWS's wording.
    ApiError::new(
        404,
        "NoSuchObjectLockConfiguration",
        "The specified object does not have a ObjectLock configuration",
    )
}

/// Event holds are a newer Object Lock feature that is not built; asking for one is refused by name, not ignored.
fn event_hold_not_built() -> ApiError {
    ApiError::new(
        501,
        "NotImplemented",
        "Object Lock event holds are not built in doze-aws.",
    )
}

fn future_date() -> ApiError {
    // (memory) AWS's wording.
    invalid_argument("The retain until date must be in the future!")
}

impl Lock {
    /// Whether this lock forbids removing the version at `now_ms`: a legal hold always does; retention does until it runs out, and
    /// GOVERNANCE retention gives way to a caller who asks to bypass it.
    pub(crate) fn blocks(&self, now_ms: i64, bypass: bool) -> bool {
        if self.legal_hold == Some(true) {
            return true;
        }
        self.retention
            .is_some_and(|r| r.until_ms > now_ms && !(bypass && r.mode == Mode::Governance))
    }
}

/// `date` (an RFC 3339 time as a request writes it) as milliseconds.
fn instant(date: &str) -> Option<i64> {
    parse_http_date(date).map(|s| s * 1000)
}

/// `ms` moved on by `years` calendar years.
fn add_years(ms: i64, years: i64) -> Option<i64> {
    let text = aws_core::time::format_iso8601(ms.div_euclid(1000));
    let (year, rest) = text.split_once('-')?;
    let moved = year.parse::<i64>().ok()?.checked_add(years)?;
    aws_core::time::parse_rfc3339(&format!("{moved:04}-{rest}")).map(|s| s * 1000)
}

/// What the Object Lock headers of a request ask for, checked. Empty when it names none.
pub(crate) fn requested(req: &Req<'_>, now_ms: i64) -> Result<Lock, ApiError> {
    if [
        "ObjectLockEventHold",
        "ObjectLockEventHoldDurationDays",
        "ObjectLockEventHoldDurationYears",
    ]
    .iter()
    .any(|m| req.get(m).is_some())
    {
        return Err(event_hold_not_built());
    }
    let retention = match (req.s("ObjectLockMode"), req.s("ObjectLockRetainUntilDate")) {
        (None, None) => None,
        (Some(mode), Some(date)) => {
            let until_ms = instant(date).ok_or_else(|| {
                invalid_argument("The retain until date is not a valid timestamp.")
            })?;
            if until_ms <= now_ms {
                return Err(future_date());
            }
            Some(Retention {
                mode: Mode::parse(mode)
                    .ok_or_else(|| invalid_argument("Unknown wormMode directive."))?,
                until_ms,
            })
        }
        // (memory) AWS's wording.
        _ => {
            return Err(invalid_argument(
                "x-amz-object-lock-retain-until-date and x-amz-object-lock-mode must both be supplied",
            ));
        }
    };
    let legal_hold = match req.s("ObjectLockLegalHoldStatus") {
        None => None,
        Some("ON") => Some(true),
        Some("OFF") => Some(false),
        Some(_) => return Err(invalid_argument("Legal Hold must be ON or OFF.")),
    };
    Ok(Lock {
        retention,
        legal_hold,
    })
}

impl Core {
    /// The lock a new object version starts with: what the request names, or else the bucket's default retention. Naming any of it
    /// on a bucket without Object Lock is refused.
    pub(crate) fn new_lock(&self, bucket: &str, asked: Lock) -> Result<Lock, ApiError> {
        let b = self.require(bucket)?;
        if !b.object_lock {
            if asked == Lock::default() {
                return Ok(asked);
            }
            return Err(missing_configuration());
        }
        let mut lock = asked;
        if lock.retention.is_none() {
            lock.retention = self.default_retention(bucket);
        }
        Ok(lock)
    }

    /// The bucket's default retention applied to an object made now.
    fn default_retention(&self, bucket: &str) -> Option<Retention> {
        let config: Value =
            serde_json::from_str(self.buckets.get(bucket)?.configs.get("object-lock")?).ok()?;
        let rule = config.pointer("/Rule/DefaultRetention")?;
        let mode = Mode::parse(rule.get("Mode")?.as_str()?)?;
        let now = self.now_ms();
        let until_ms = match (
            rule.get("Days").and_then(Value::as_i64),
            rule.get("Years").and_then(Value::as_i64),
        ) {
            (Some(d), _) => now.checked_add(d.checked_mul(DAY_MS)?)?,
            (None, Some(y)) => add_years(now, y)?,
            (None, None) => return None,
        };
        Some(Retention { mode, until_ms })
    }

    /// Refuses a removal of `v` that its lock forbids.
    pub(crate) fn guard_removal(&self, v: &Version, bypass: bool) -> Result<(), ApiError> {
        if !v.delete_marker && v.lock.blocks(self.now_ms(), bypass) {
            return Err(locked());
        }
        Ok(())
    }

    /// Refuses a write that would replace the `null` version of `key` while that is locked (versioning off or suspended replaces it).
    pub(crate) fn guard_replace(&self, bucket: &str, key: &str) -> Result<(), ApiError> {
        if self.require(bucket)?.versioning == crate::model::Versioning::Enabled {
            return Ok(());
        }
        for old in self
            .versions_of(bucket, key)?
            .iter()
            .filter(|v| v.version_id == "null")
        {
            self.guard_removal(old, false)?;
        }
        Ok(())
    }

    /// A bucket that has Object Lock, or the refusal retention and holds give otherwise.
    fn locking(&self, bucket: &str) -> Result<(), ApiError> {
        if self.require(bucket)?.object_lock {
            Ok(())
        } else {
            Err(missing_configuration())
        }
    }

    pub(crate) fn get_retention(&self, req: &Req<'_>) -> Answer {
        self.locking(req.bucket())?;
        let v = self.readable(req.bucket(), req.key(), req.s("VersionId"))?;
        let r = v.lock.retention.ok_or_else(no_object_lock)?;
        Out::json(
            json!({"Retention": {"Mode": r.mode.name(), "RetainUntilDate": r.until_ms.div_euclid(1000)}}),
        )
    }

    pub(crate) fn put_retention(&mut self, req: &Req<'_>) -> Answer {
        let (bucket, key) = (req.bucket(), req.key());
        self.locking(bucket)?;
        let mut v = self.readable(bucket, key, req.s("VersionId"))?;
        let now = self.now_ms();
        let new = match req.get("Retention") {
            None => None,
            Some(r) => {
                if r.get("EventHold").is_some() || r.get("EventHoldDuration").is_some() {
                    return Err(event_hold_not_built());
                }
                match (
                    r.get("Mode").and_then(Value::as_str),
                    r.get("RetainUntilDate").and_then(Value::as_str),
                ) {
                    (None, None) => None,
                    (Some(mode), Some(date)) => {
                        let until_ms = instant(date).ok_or_else(malformed_xml)?;
                        if until_ms <= now {
                            return Err(future_date());
                        }
                        Some(Retention {
                            mode: Mode::parse(mode).ok_or_else(malformed_xml)?,
                            until_ms,
                        })
                    }
                    _ => return Err(malformed_xml()),
                }
            }
        };
        if let Some(old) = v.lock.retention.filter(|o| o.until_ms > now) {
            // Making retention stricter is always allowed; making it weaker (shorter, removed, or COMPLIANCE turned to GOVERNANCE)
            // is never allowed for COMPLIANCE, and for GOVERNANCE only to a caller who bypasses it.
            let weaker = new.is_none_or(|n| {
                n.until_ms < old.until_ms
                    || (old.mode == Mode::Compliance && n.mode == Mode::Governance)
            });
            let bypass = req.flag("BypassGovernanceRetention") == Some(true);
            if weaker && (old.mode == Mode::Compliance || !bypass) {
                return Err(locked());
            }
        }
        v.lock.retention = new;
        self.put_row(bucket, &v)?;
        Out::empty()
    }

    pub(crate) fn get_legal_hold(&self, req: &Req<'_>) -> Answer {
        self.locking(req.bucket())?;
        let v = self.readable(req.bucket(), req.key(), req.s("VersionId"))?;
        let on = v.lock.legal_hold.ok_or_else(no_object_lock)?;
        Out::json(json!({"LegalHold": {"Status": if on { "ON" } else { "OFF" }}}))
    }

    pub(crate) fn put_legal_hold(&mut self, req: &Req<'_>) -> Answer {
        let (bucket, key) = (req.bucket(), req.key());
        self.locking(bucket)?;
        let mut v = self.readable(bucket, key, req.s("VersionId"))?;
        let on = match req
            .get("LegalHold")
            .and_then(|h| h.get("Status"))
            .and_then(Value::as_str)
        {
            Some("ON") => true,
            Some("OFF") => false,
            _ => return Err(malformed_xml()),
        };
        v.lock.legal_hold = Some(on);
        self.put_row(bucket, &v)?;
        Out::empty()
    }

    /// The Object Lock settings an upload was started with, kept until it completes.
    pub(crate) fn upload_lock(&self, row: &str) -> Lock {
        self.upload_locks.get(row).copied().unwrap_or_default()
    }

    /// `GetBucketPolicyStatus`: whether the stored policy makes the bucket public.
    pub(crate) fn get_policy_status(&self, req: &Req<'_>) -> Answer {
        let text = self
            .require(req.bucket())?
            .configs
            .get("policy")
            .ok_or_else(|| no_config("policy"))?;
        let doc: Value = serde_json::from_str(text).unwrap_or(Value::Null);
        Out::json(json!({"PolicyStatus": {"IsPublic": crate::validate::grants_public(&doc)}}))
    }
}

/// The headers that tell what lock a version has, as output members.
pub(crate) fn describe(lock: &Lock, out: &mut serde_json::Map<String, Value>) {
    if let Some(r) = lock.retention {
        out.insert("ObjectLockMode".into(), json!(r.mode.name()));
        // WHY: a string, not the number the model's date-time would be written from: a header written from a number would be an
        // HTTP date, and S3 writes this one as ISO 8601. (memory)
        out.insert(
            "ObjectLockRetainUntilDate".into(),
            json!(aws_core::time::format_iso8601(r.until_ms.div_euclid(1000))),
        );
    }
    if let Some(on) = lock.legal_hold {
        out.insert(
            "ObjectLockLegalHoldStatus".into(),
            json!(if on { "ON" } else { "OFF" }),
        );
    }
}

/// The checks of a bucket's Object Lock configuration (`PutObjectLockConfiguration`).
pub(crate) fn check_configuration(
    core: &Core,
    req: &Req<'_>,
    body: &Value,
) -> Result<(), ApiError> {
    let b = core.require(req.bucket())?;
    if body.get("ObjectLockEnabled").and_then(Value::as_str) != Some("Enabled") {
        return Err(malformed_xml());
    }
    if let Some(rule) = body.pointer("/Rule/DefaultRetention") {
        if rule.get("DefaultEventHold").is_some() {
            return Err(event_hold_not_built());
        }
        let days = rule.get("Days").and_then(Value::as_i64);
        let years = rule.get("Years").and_then(Value::as_i64);
        let one_period = match (days, years) {
            (Some(n), None) | (None, Some(n)) => n >= 1,
            _ => false,
        };
        if rule.get("Mode").and_then(Value::as_str).is_none() || !one_period {
            return Err(malformed_xml());
        }
    }
    if !b.object_lock && b.versioning != crate::model::Versioning::Enabled {
        // (memory) AWS's wording.
        return Err(crate::errors::invalid_bucket_state(
            "Versioning must be 'Enabled' on the bucket to apply a Object Lock configuration",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lock(mode: Mode, until_ms: i64) -> Lock {
        Lock {
            retention: Some(Retention { mode, until_ms }),
            legal_hold: None,
        }
    }

    #[test]
    fn what_a_lock_forbids() {
        let now = 1_000;
        assert!(!Lock::default().blocks(now, false));
        assert!(
            lock(Mode::Compliance, 2_000).blocks(now, true),
            "no bypass for COMPLIANCE"
        );
        assert!(lock(Mode::Governance, 2_000).blocks(now, false));
        assert!(!lock(Mode::Governance, 2_000).blocks(now, true));
        assert!(
            !lock(Mode::Compliance, 1_000).blocks(now, false),
            "retention that ran out"
        );
        let hold = Lock {
            retention: None,
            legal_hold: Some(true),
        };
        assert!(hold.blocks(now, true), "a legal hold is not bypassed");
        assert!(
            !Lock {
                legal_hold: Some(false),
                ..hold
            }
            .blocks(now, false)
        );
        let both = Lock {
            legal_hold: Some(true),
            ..lock(Mode::Governance, 2_000)
        };
        assert!(
            both.blocks(now, true),
            "bypassing retention leaves the hold"
        );
    }

    #[test]
    fn years_are_calendar_years() {
        // 2024-02-29T00:00:00Z plus one year is 2025-03-01.
        assert_eq!(add_years(1_709_164_800_000, 1), Some(1_740_787_200_000));
        assert_eq!(add_years(0, 1), Some(31_536_000_000));
    }
}
