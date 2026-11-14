//! Lifecycle rules, enforced: expiration of current versions (a delete marker where the bucket is versioned), removal of noncurrent
//! versions, removal of expired delete markers, and aborting stale multipart uploads. A rule selects objects by prefix, tag, and
//! size, singly or together under `And`, and selects exactly those: a rule with a tag filter touches only tagged objects (the Go
//! edition widened any rule that was not a plain prefix to the whole bucket). Transitions are stored and not acted on: there is
//! one storage tier here.

use serde_json::Value;

use crate::core::Core;
use crate::model::Version;

const DAY_MS: i64 = 86_400_000;

/// One enabled rule, read from the stored configuration.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Rule {
    pub(crate) prefix: String,
    pub(crate) tags: Vec<(String, String)>,
    pub(crate) larger_than: Option<u64>,
    pub(crate) smaller_than: Option<u64>,
    pub(crate) expire_days: Option<i64>,
    pub(crate) expire_date_ms: Option<i64>,
    pub(crate) expired_marker: bool,
    pub(crate) noncurrent_days: Option<i64>,
    pub(crate) keep_noncurrent: Option<usize>,
    pub(crate) abort_days: Option<i64>,
}

/// The enabled rules of a stored lifecycle configuration.
pub(crate) fn rules_of(config: &str) -> Vec<Rule> {
    let doc: Value = serde_json::from_str(config).unwrap_or(Value::Null);
    doc.get("Rules")
        .and_then(Value::as_array)
        .map(|rs| {
            rs.iter()
                .filter(|r| r.get("Status").and_then(Value::as_str) == Some("Enabled"))
                .map(rule)
                .collect()
        })
        .unwrap_or_default()
}

fn rule(r: &Value) -> Rule {
    let filter = r.get("Filter");
    let and = filter.and_then(|f| f.get("And"));
    let pick = |name: &str| {
        and.and_then(|a| a.get(name))
            .or_else(|| filter.and_then(|f| f.get(name)))
    };
    let mut tags: Vec<(String, String)> = and
        .and_then(|a| a.get("Tags"))
        .and_then(Value::as_array)
        .map(|t| t.iter().filter_map(tag).collect())
        .unwrap_or_default();
    tags.extend(filter.and_then(|f| f.get("Tag")).and_then(tag));
    let int = |v: Option<&Value>| v.and_then(Value::as_i64);
    let nc = r.get("NoncurrentVersionExpiration");
    Rule {
        prefix: pick("Prefix")
            .and_then(Value::as_str)
            .or_else(|| r.get("Prefix").and_then(Value::as_str))
            .unwrap_or_default()
            .to_owned(),
        tags,
        larger_than: int(pick("ObjectSizeGreaterThan")).and_then(|n| u64::try_from(n).ok()),
        smaller_than: int(pick("ObjectSizeLessThan")).and_then(|n| u64::try_from(n).ok()),
        expire_days: int(r.pointer("/Expiration/Days")),
        expire_date_ms: r
            .pointer("/Expiration/Date")
            .and_then(Value::as_str)
            .and_then(crate::timeutil::parse_http_date)
            .map(|s| s * 1000),
        expired_marker: r
            .pointer("/Expiration/ExpiredObjectDeleteMarker")
            .and_then(Value::as_bool)
            == Some(true),
        noncurrent_days: int(nc.and_then(|n| n.get("NoncurrentDays"))),
        keep_noncurrent: int(nc.and_then(|n| n.get("NewerNoncurrentVersions")))
            .and_then(|n| usize::try_from(n).ok()),
        abort_days: int(r.pointer("/AbortIncompleteMultipartUpload/DaysAfterInitiation")),
    }
}

fn tag(t: &Value) -> Option<(String, String)> {
    Some((
        t.get("Key")?.as_str()?.to_owned(),
        t.get("Value")?.as_str()?.to_owned(),
    ))
}

impl Rule {
    /// Whether the rule selects an object of this key, size and tag set.
    pub(crate) fn selects(&self, key: &str, size: u64, tags: &[(String, String)]) -> bool {
        key.starts_with(&self.prefix)
            && self.tags.iter().all(|t| tags.contains(t))
            && self.larger_than.is_none_or(|n| size > n)
            && self.smaller_than.is_none_or(|n| size < n)
    }
}

/// When an object last modified at `modified_ms` is due after `days`: the next UTC midnight at or after that many days on.
pub(crate) fn due_ms(modified_ms: i64, days: i64) -> i64 {
    let at = modified_ms + days * DAY_MS;
    (at + DAY_MS - 1).div_euclid(DAY_MS) * DAY_MS
}

/// What a sweep decided to do.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Action {
    /// Expire the current version of the key (a delete marker, or removal, as the bucket's versioning says).
    Expire(String),
    /// Remove this version for good.
    Remove(String, String),
    /// Abort this upload (by its row).
    Abort(String),
}

/// The actions the rules call for on one key's versions (newest first) at `now_ms`.
pub(crate) fn decide(rules: &[Rule], versions: &[Version], now_ms: i64) -> Vec<Action> {
    let mut out = Vec::new();
    let Some(current) = versions.first() else {
        return out;
    };
    for r in rules {
        let wants = |v: &Version| r.selects(&v.key, v.size, &v.tags);
        if !current.delete_marker && wants(current) {
            let by_days = r
                .expire_days
                .is_some_and(|d| now_ms >= due_ms(current.modified_ms, d));
            if by_days || r.expire_date_ms.is_some_and(|t| now_ms >= t) {
                out.push(Action::Expire(current.key.clone()));
            }
        }
        if current.delete_marker
            && r.expired_marker
            && versions.len() == 1
            && r.selects(&current.key, 0, &[])
        {
            out.push(Action::Remove(
                current.key.clone(),
                current.version_id.clone(),
            ));
        }
        noncurrent(r, versions, now_ms, &mut out);
    }
    out
}

/// Noncurrent versions become so when the next newer version is written; they go `NoncurrentDays` after that, and the newest
/// `NewerNoncurrentVersions` of them stay regardless.
fn noncurrent(r: &Rule, versions: &[Version], now_ms: i64, out: &mut Vec<Action>) {
    let Some(days) = r.noncurrent_days else {
        return;
    };
    for (i, older) in versions.iter().enumerate().skip(1) {
        let Some(newer) = versions.get(i - 1) else {
            continue;
        };
        if r.keep_noncurrent.is_some_and(|n| i <= n) {
            continue;
        }
        let selected = r.selects(&older.key, older.size, &older.tags);
        if selected && now_ms >= due_ms(newer.modified_ms, days) {
            out.push(Action::Remove(older.key.clone(), older.version_id.clone()));
        }
    }
}

impl Core {
    /// Applies every enabled rule of every bucket as of now. Returns how many things it did.
    pub(crate) fn sweep_lifecycle(&mut self) -> usize {
        let now = self.now_ms();
        let buckets: Vec<(String, Vec<Rule>)> = self
            .buckets
            .iter()
            .filter_map(|(n, b)| Some((n.to_owned(), rules_of(b.configs.get("lifecycle")?))))
            .filter(|(_, r)| !r.is_empty())
            .collect();
        let mut done = 0;
        for (bucket, rules) in buckets {
            let mut actions = self.lifecycle_actions(&bucket, &rules, now);
            actions.extend(self.stale_uploads(&bucket, &rules, now));
            for a in actions {
                done += usize::from(self.apply(&bucket, &a));
            }
        }
        done
    }

    /// Reads every key of the bucket and decides what the rules do to it.
    fn lifecycle_actions(&self, bucket: &str, rules: &[Rule], now: i64) -> Vec<Action> {
        let q = crate::list::Query {
            bucket,
            prefix: "",
            delimiter: None,
            after: "",
            max: usize::MAX,
        };
        let mut actions = Vec::new();
        let mut group: Vec<Version> = Vec::new();
        let flush = |group: &mut Vec<Version>, actions: &mut Vec<Action>| {
            actions.extend(decide(rules, group, now));
            group.clear();
        };
        let walked = self.walk(&q, |key, bytes| {
            if group.first().is_some_and(|g| g.key != key) {
                flush(&mut group, &mut actions);
            }
            if let Ok(v) = crate::store::decode_version(bytes) {
                group.push(v);
            }
            true
        });
        if walked.is_ok() {
            flush(&mut group, &mut actions);
        }
        actions
    }

    fn stale_uploads(&self, bucket: &str, rules: &[Rule], now: i64) -> Vec<Action> {
        let prefix = format!("{bucket}\0");
        self.uploads
            .with_prefix(&prefix)
            .filter(|(_, u)| {
                rules.iter().any(|r| {
                    r.abort_days
                        .is_some_and(|d| now >= due_ms(u.initiated_ms, d))
                        && u.key.starts_with(&r.prefix)
                        && r.tags.is_empty()
                })
            })
            .map(|(row, _)| Action::Abort(row.to_owned()))
            .collect()
    }

    fn apply(&mut self, bucket: &str, action: &Action) -> bool {
        match action {
            Action::Expire(key) => self
                .remove_key(bucket, key, None, false)
                .map(|r| self.note_lifecycle(bucket, key, &r))
                .is_ok(),
            Action::Remove(key, id) => self
                .remove_key(bucket, key, Some(id), false)
                .map(|r| self.note_lifecycle(bucket, key, &r))
                .is_ok(),
            Action::Abort(row) => {
                self.remove_upload(row);
                true
            }
        }
    }

    /// Whether any bucket has an enabled lifecycle rule (the janitor stays armed while one does).
    pub(crate) fn has_lifecycle_rules(&self) -> bool {
        self.buckets.iter().any(|(_, b)| {
            b.configs
                .get("lifecycle")
                .is_some_and(|c| !rules_of(c).is_empty())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn version(
        key: &str,
        id: &str,
        modified_days: i64,
        size: u64,
        tags: &[(&str, &str)],
        marker: bool,
    ) -> Version {
        Version {
            key: key.into(),
            version_id: id.into(),
            seq: 0,
            delete_marker: marker,
            modified_ms: modified_days * DAY_MS,
            size,
            etag: String::new(),
            meta: crate::model::Meta::default(),
            checksum: None,
            segs: Vec::new(),
            parts: Vec::new(),
            tags: tags
                .iter()
                .map(|(k, v)| ((*k).into(), (*v).into()))
                .collect(),
            lock: crate::model::Lock::default(),
            acl: None,
        }
    }

    fn rules(doc: Value) -> Vec<Rule> {
        rules_of(&doc.to_string())
    }

    #[test]
    fn a_rule_selects_by_prefix_tag_and_size_and_never_wider() {
        let tagged = rules(
            json!({"Rules": [{"Status": "Enabled", "Filter": {"Tag": {"Key": "t", "Value": "1"}}, "Expiration": {"Days": 1}}]}),
        );
        let old = |tags: &[(&str, &str)]| vec![version("k", "null", 0, 10, tags, false)];
        let day = 10 * DAY_MS;
        assert_eq!(
            decide(&tagged, &old(&[("t", "1")]), day),
            [Action::Expire("k".into())]
        );
        assert!(
            decide(&tagged, &old(&[]), day).is_empty(),
            "an untagged object is not selected by a tag rule"
        );
        assert!(decide(&tagged, &old(&[("t", "2")]), day).is_empty());

        let and = rules(
            json!({"Rules": [{"Status": "Enabled", "Filter": {"And": {"Prefix": "logs/", "ObjectSizeGreaterThan": 5, "Tags": [{"Key": "t", "Value": "1"}]}}, "Expiration": {"Days": 1}}]}),
        );
        let at = |key: &str, size: u64, tags: &[(&str, &str)]| {
            vec![version(key, "null", 0, size, tags, false)]
        };
        assert_eq!(decide(&and, &at("logs/a", 6, &[("t", "1")]), day).len(), 1);
        assert!(
            decide(&and, &at("logs/a", 5, &[("t", "1")]), day).is_empty(),
            "the size bound is exclusive"
        );
        assert!(decide(&and, &at("other/a", 6, &[("t", "1")]), day).is_empty());
        assert!(decide(&and, &at("logs/a", 6, &[]), day).is_empty());
        let small = rules(
            json!({"Rules": [{"Status": "Enabled", "Filter": {"ObjectSizeLessThan": 5}, "Expiration": {"Days": 1}}]}),
        );
        assert_eq!(decide(&small, &at("k", 4, &[]), day).len(), 1);
        assert!(decide(&small, &at("k", 5, &[]), day).is_empty());
    }

    #[test]
    fn days_count_to_the_next_midnight() {
        let r = rules(
            json!({"Rules": [{"Status": "Enabled", "Filter": {"Prefix": ""}, "Expiration": {"Days": 1}}]}),
        );
        let mut v = version("k", "null", 5, 1, &[], false);
        v.modified_ms += 3_600_000; // 01:00 on day 5
        assert!(
            decide(&r, std::slice::from_ref(&v), 6 * DAY_MS + 3_600_000).is_empty(),
            "a day on is not yet midnight after it"
        );
        assert_eq!(decide(&r, std::slice::from_ref(&v), 7 * DAY_MS).len(), 1);
        let disabled = rules(
            json!({"Rules": [{"Status": "Disabled", "Filter": {}, "Expiration": {"Days": 1}}]}),
        );
        assert!(disabled.is_empty());
    }

    #[test]
    fn noncurrent_versions_go_after_their_days_and_markers_when_alone() {
        let r = rules(
            json!({"Rules": [{"Status": "Enabled", "Filter": {}, "NoncurrentVersionExpiration": {"NoncurrentDays": 2}, "Expiration": {"ExpiredObjectDeleteMarker": true}}]}),
        );
        let versions = [
            version("k", "v3", 9, 1, &[], false),
            version("k", "v2", 5, 1, &[], false),
            version("k", "v1", 1, 1, &[], false),
        ];
        // v2 became noncurrent when v3 was written on day 9; v1 when v2 was, on day 5.
        let acts = decide(&r, &versions, 10 * DAY_MS);
        assert_eq!(acts, [Action::Remove("k".into(), "v1".into())]);
        let later = decide(&r, &versions, 12 * DAY_MS);
        assert_eq!(later.len(), 2);
        let lone = [version("m", "d1", 1, 0, &[], true)];
        assert_eq!(
            decide(&r, &lone, DAY_MS),
            [Action::Remove("m".into(), "d1".into())]
        );
    }

    #[test]
    fn the_newest_noncurrent_versions_can_be_kept() {
        let r = rules(
            json!({"Rules": [{"Status": "Enabled", "Filter": {}, "NoncurrentVersionExpiration": {"NoncurrentDays": 1, "NewerNoncurrentVersions": 1}}]}),
        );
        let versions = [
            version("k", "v3", 9, 1, &[], false),
            version("k", "v2", 5, 1, &[], false),
            version("k", "v1", 1, 1, &[], false),
        ];
        assert_eq!(
            decide(&r, &versions, 20 * DAY_MS),
            [Action::Remove("k".into(), "v1".into())],
            "v2 is the one noncurrent version kept"
        );
    }
}
