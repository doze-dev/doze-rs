//! Looking at secrets without reading their values: `DescribeSecret`, `ListSecrets`, `ListSecretVersionIds`.

use aws_model_secrets_manager::keys::{
    DescribeSecretRequest as Describe, DescribeSecretResponse as Described, Filter,
    ListSecretVersionIdsRequest as Versions, ListSecretVersionIdsResponse as Listed,
    ListSecretsRequest as List, ListSecretsResponse as Secrets, RotationRulesType as Rules,
    SecretListEntry as Entry, SecretVersionsListEntry as VersionEntry, Tag,
};
use aws_schema::build::Object;
use aws_wire::handler::{reply, request};
use aws_wire::service::{Call, Reply};

use crate::core::{Core, invalid_parameter};
use crate::model::{Rotation, Secret};

const DEFAULT_KEY: &str = "DefaultEncryptionKey";
const LIST_PAGE: usize = 100;
const VERSIONS_PAGE: usize = 100;

#[allow(clippy::cast_precision_loss)]
fn seconds(ms: i64) -> f64 {
    ms as f64 / 1000.0
}

/// One filter of `ListSecrets`: a key, and the values to match (a leading `!` negates one).
struct Cond {
    key: String,
    values: Vec<String>,
}

impl Cond {
    /// Whether `s` passes. Unrecorded: a value matches when the text, or a word of it, starts with it, ignoring case (AWS's
    /// words are "prefix match"); `name` is a plain prefix.
    fn holds(&self, s: &Secret) -> bool {
        let starts = |text: &str, want: &str| {
            let (text, want) = (text.to_lowercase(), want.to_lowercase());
            text.starts_with(&want)
                || text
                    .split(|c: char| !c.is_alphanumeric())
                    .any(|w| w.starts_with(&want))
        };
        let any = |texts: &[&str]| {
            self.values.iter().any(|v| match v.strip_prefix('!') {
                Some(neg) => !texts.iter().any(|t| starts(t, neg)),
                None => texts.iter().any(|t| starts(t, v)),
            })
        };
        let tag_keys: Vec<&str> = s.tags.keys().map(String::as_str).collect();
        let tag_values: Vec<&str> = s.tags.values().map(String::as_str).collect();
        match self.key.as_str() {
            "name" => self.values.iter().any(|v| match v.strip_prefix('!') {
                Some(neg) => !s.name.starts_with(neg),
                None => s.name.starts_with(v.as_str()),
            }),
            "description" => any(&[s.description.as_deref().unwrap_or_default()]),
            "tag-key" => any(&tag_keys),
            "tag-value" => any(&tag_values),
            "all" => {
                any(&[&s.name, s.description.as_deref().unwrap_or_default()])
                    || any(&tag_keys)
                    || any(&tag_values)
            }
            _ => false,
        }
    }
}

fn describe_into(s: &Secret, r: Option<&Rotation>, d: &mut Object<'_, Described>) {
    d.put(Described::ARN, &s.arn);
    d.put(Described::NAME, &s.name);
    if let Some(t) = &s.kind {
        d.put(Described::TYPE, t);
    }
    if let Some(x) = &s.description {
        d.put(Described::DESCRIPTION, x);
    }
    if let Some(k) = &s.kms_key_id {
        d.put(Described::KMS_KEY_ID, k);
    }
    d.put_time(Described::LAST_CHANGED_DATE, seconds(s.changed_ms));
    if let Some(r) = r {
        d.put_bool(Described::ROTATION_ENABLED, r.enabled);
        if let Some(f) = &r.function {
            d.put(Described::ROTATION_LAMBDA_ARN, f);
        }
        if r.after_days.is_some() || r.schedule.is_some() || r.duration.is_some() {
            d.put_object(Described::ROTATION_RULES, |o| rules_into(r, o));
        }
        if let Some(at) = r.last_ms {
            d.put_time(Described::LAST_ROTATED_DATE, seconds(at));
        }
        if let Some(at) = r.next_ms() {
            d.put_time(Described::NEXT_ROTATION_DATE, seconds(at));
        }
    }
    if let Some(a) = s.accessed_day_ms {
        d.put_time(Described::LAST_ACCESSED_DATE, seconds(a));
    }
    if let Some(x) = s.deleted_ms {
        d.put_time(Described::DELETED_DATE, seconds(x));
    }
    if !s.tags.is_empty() {
        d.put_list(Described::TAGS, |l| {
            for (k, v) in &s.tags {
                l.push(|t: &mut Object<'_, Tag>| {
                    t.put(Tag::KEY, k);
                    t.put(Tag::VALUE, v);
                });
            }
        });
    }
    if s.labelled().next().is_some() {
        d.put_map(Described::VERSION_IDS_TO_STAGES, |m| {
            for (id, v) in s.labelled() {
                m.insert(id, |l| {
                    for stage in &v.stages {
                        l.push(stage);
                    }
                });
            }
        });
    }
    d.put_time(Described::CREATED_DATE, seconds(s.created_ms));
}

fn rules_into(r: &Rotation, o: &mut Object<'_, Rules>) {
    if let Some(n) = r.after_days {
        o.put_int(Rules::AUTOMATICALLY_AFTER_DAYS, n);
    }
    if let Some(x) = &r.duration {
        o.put(Rules::DURATION, x);
    }
    if let Some(x) = &r.schedule {
        o.put(Rules::SCHEDULE_EXPRESSION, x);
    }
}

fn list_entry(s: &Secret, r: Option<&Rotation>, d: &mut Object<'_, Entry>) {
    d.put(Entry::ARN, &s.arn);
    d.put(Entry::NAME, &s.name);
    if let Some(t) = &s.kind {
        d.put(Entry::TYPE, t);
    }
    if let Some(x) = &s.description {
        d.put(Entry::DESCRIPTION, x);
    }
    if let Some(k) = &s.kms_key_id {
        d.put(Entry::KMS_KEY_ID, k);
    }
    d.put_time(Entry::LAST_CHANGED_DATE, seconds(s.changed_ms));
    if let Some(r) = r {
        d.put_bool(Entry::ROTATION_ENABLED, r.enabled);
        if let Some(f) = &r.function {
            d.put(Entry::ROTATION_LAMBDA_ARN, f);
        }
        if r.after_days.is_some() || r.schedule.is_some() || r.duration.is_some() {
            d.put_object(Entry::ROTATION_RULES, |o| rules_into(r, o));
        }
        if let Some(at) = r.last_ms {
            d.put_time(Entry::LAST_ROTATED_DATE, seconds(at));
        }
        if let Some(at) = r.next_ms() {
            d.put_time(Entry::NEXT_ROTATION_DATE, seconds(at));
        }
    }
    if let Some(a) = s.accessed_day_ms {
        d.put_time(Entry::LAST_ACCESSED_DATE, seconds(a));
    }
    if let Some(x) = s.deleted_ms {
        d.put_time(Entry::DELETED_DATE, seconds(x));
    }
    if !s.tags.is_empty() {
        d.put_list(Entry::TAGS, |l| {
            for (k, v) in &s.tags {
                l.push(|t: &mut Object<'_, Tag>| {
                    t.put(Tag::KEY, k);
                    t.put(Tag::VALUE, v);
                });
            }
        });
    }
    if s.labelled().next().is_some() {
        d.put_map(Entry::SECRET_VERSIONS_TO_STAGES, |m| {
            for (id, v) in s.labelled() {
                m.insert(id, |l| {
                    for stage in &v.stages {
                        l.push(stage);
                    }
                });
            }
        });
    }
    d.put_time(Entry::CREATED_DATE, seconds(s.created_ms));
}

impl Core {
    pub(crate) fn describe_secret(&mut self, call: &Call) -> Reply {
        let v = request::<Describe>(call)?;
        let s = self.secret(v.get(Describe::SECRET_ID).unwrap_or_default())?;
        let r = self.rotations.get(&s.name);
        reply::<Described>(|d| describe_into(s, r, d))
    }

    pub(crate) fn list_secrets(&mut self, call: &Call) -> Reply {
        let v = request::<List>(call)?;
        let include_deleted = v.get(List::INCLUDE_PLANNED_DELETION).unwrap_or(false);
        let limit = v
            .get(List::MAX_RESULTS)
            .map_or(LIST_PAGE, |n| usize::try_from(n).unwrap_or(LIST_PAGE));
        let start = match v.get(List::NEXT_TOKEN) {
            Some(t) => crate::token::resume(t)?,
            None => 0,
        };
        let conds: Vec<Cond> = v
            .get(List::FILTERS)
            .into_iter()
            .flatten()
            .map(|f| Cond {
                key: f.get(Filter::KEY).unwrap_or_default().to_owned(),
                values: f
                    .get(Filter::VALUES)
                    .map(|x| x.map(str::to_owned).collect())
                    .unwrap_or_default(),
            })
            .collect();
        if let Some(bad) = conds.iter().find(|c| {
            ![
                "name",
                "description",
                "tag-key",
                "tag-value",
                "primary-region",
                "owning-service",
                "all",
            ]
            .contains(&c.key.as_str())
        }) {
            return Err(invalid_parameter(format!(
                "Invalid filter key: {}",
                bad.key
            )));
        }
        let mut rows: Vec<&Secret> = self
            .secrets
            .iter()
            .map(|(_, s)| s)
            .filter(|s| include_deleted || s.deleted_ms.is_none())
            .filter(|s| conds.iter().all(|c| c.holds(s)))
            .collect();
        match v.get(List::SORT_BY).unwrap_or("name") {
            "created-date" => rows.sort_by_key(|s| s.created_ms),
            "last-changed-date" => rows.sort_by_key(|s| s.changed_ms),
            "last-accessed-date" => rows.sort_by_key(|s| s.accessed_day_ms),
            _ => {}
        }
        if v.get(List::SORT_ORDER) == Some("desc") {
            rows.reverse();
        }
        let more = rows.len() > start + limit;
        let page: Vec<&Secret> = rows.into_iter().skip(start).take(limit).collect();
        let token = more.then(|| crate::token::of(start + limit));
        reply::<Secrets>(|o| {
            o.put_list(Secrets::SECRET_LIST, |l| {
                for s in &page {
                    l.push(|d| list_entry(s, self.rotations.get(&s.name), d));
                }
            });
            if let Some(t) = &token {
                o.put(Secrets::NEXT_TOKEN, t);
            }
        })
    }

    pub(crate) fn list_secret_version_ids(&mut self, call: &Call) -> Reply {
        let v = request::<Versions>(call)?;
        let s = self.secret(v.get(Versions::SECRET_ID).unwrap_or_default())?;
        let deprecated = v.get(Versions::INCLUDE_DEPRECATED).unwrap_or(false);
        let limit = v.get(Versions::MAX_RESULTS).map_or(VERSIONS_PAGE, |n| {
            usize::try_from(n).unwrap_or(VERSIONS_PAGE)
        });
        let start = match v.get(Versions::NEXT_TOKEN) {
            Some(t) => crate::token::resume(t)?,
            None => 0,
        };
        let key = s.kms_key_id.as_deref().unwrap_or(DEFAULT_KEY);
        let mut rows: Vec<_> = s
            .versions
            .iter()
            .filter(|(_, x)| deprecated || !x.stages.is_empty())
            .collect();
        rows.sort_by_key(|(_, x)| x.created_ms);
        let more = rows.len() > start + limit;
        let page: Vec<_> = rows.into_iter().skip(start).take(limit).collect();
        let token = more.then(|| crate::token::of(start + limit));
        reply::<Listed>(|o| {
            o.put_list(Listed::VERSIONS, |l| {
                for (id, x) in &page {
                    l.push(|e: &mut Object<'_, VersionEntry>| {
                        e.put(VersionEntry::VERSION_ID, id);
                        e.put_list(VersionEntry::VERSION_STAGES, |st| {
                            for stage in &x.stages {
                                st.push(stage);
                            }
                        });
                        if let Some(a) = x.accessed_day_ms {
                            e.put_time(VersionEntry::LAST_ACCESSED_DATE, seconds(a));
                        }
                        e.put_time(VersionEntry::CREATED_DATE, seconds(x.created_ms));
                        e.put_list(VersionEntry::KMS_KEY_IDS, |k| {
                            k.push(key);
                        });
                    });
                }
            });
            if let Some(t) = &token {
                o.put(Listed::NEXT_TOKEN, t);
            }
            o.put(Listed::ARN, &s.arn);
            o.put(Listed::NAME, &s.name);
        })
    }
}
