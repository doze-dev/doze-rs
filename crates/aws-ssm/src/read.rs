//! Reading one parameter, or the names given: `GetParameter`, `GetParameters`, `GetParameterHistory`.

use aws_model_ssm::keys::{
    GetParameterHistoryRequest as HistoryRequest, GetParameterHistoryResult as History,
    GetParameterRequest as One, GetParameterResult as OneResult, GetParametersRequest as Many,
    GetParametersResult as ManyResult, Parameter as Shown, ParameterHistory as Past,
    ParameterInlinePolicy as Inline,
};
use aws_schema::build::Object;
use aws_wire::ApiError;
use aws_wire::service::{Call, Reply};

use crate::core::{Core, reply, request};
use crate::model::{Parameter, Version};
use crate::page;
use crate::write::not_found;

/// The most versions one page of history holds.
const HISTORY_PAGE: usize = 50;

pub(crate) fn version_not_found(name: &str, which: &str) -> ApiError {
    // Recorded for a version number; the same words are used for a label (not recorded).
    ApiError::modeled(
        "ParameterVersionNotFound",
        format!(
            "Systems Manager could not find version {which} of {name}. Verify the version and try again."
        ),
    )
}

impl Core {
    /// The value as the API shows it: sealed (as hex) for a `SecureString` unless the caller asked for it open.
    pub(crate) fn reveal(&self, v: &Version, decrypt: bool) -> Result<String, ApiError> {
        if v.kind != crate::model::Kind::SecureString {
            return Ok(String::from_utf8_lossy(&v.value).into_owned());
        }
        if !decrypt {
            return Ok(aws_core::hex::encode(&v.value));
        }
        let plain = self
            .sealer
            .open(&v.value)
            .ok_or_else(|| ApiError::internal("a stored SecureString could not be opened"))?;
        Ok(String::from_utf8_lossy(&plain).into_owned())
    }

    /// The version a name with an optional `:version` or `:label` stands for.
    fn pick<'a>(
        &'a self,
        selector: &name::Selector<'a>,
    ) -> Result<(&'a Parameter, &'a Version), ApiError> {
        let p = self.params.get(selector.name).ok_or_else(not_found)?;
        let v = match selector.qualifier {
            None => &p.latest,
            Some(q) => match q.parse::<i64>() {
                Ok(n) => p.version(n),
                Err(_) => p.labelled(q),
            }
            .ok_or_else(|| version_not_found(selector.name, q))?,
        };
        Ok((p, v))
    }

    pub(crate) fn get_parameter(&mut self, call: &Call) -> Reply {
        let v = request::<One>(call)?;
        let text = v.get(One::NAME).unwrap_or_default();
        let selector = name::selector(text);
        let (_, version) = self.pick(&selector)?;
        let value = self.reveal(version, v.get(One::WITH_DECRYPTION).unwrap_or(false))?;
        reply::<OneResult>(|o| {
            o.put_object(OneResult::PARAMETER, |p| {
                shown(p, call, selector.name, version, selector.qualifier, &value)
            });
        })
    }

    pub(crate) fn get_parameters(&mut self, call: &Call) -> Reply {
        let v = request::<Many>(call)?;
        let decrypt = v.get(Many::WITH_DECRYPTION).unwrap_or(false);
        let names: Vec<&str> = v
            .get(Many::NAMES)
            .map(Iterator::collect)
            .unwrap_or_default();
        let mut found = Vec::new();
        let mut missing = Vec::new();
        for text in names {
            let selector = name::selector(text);
            match self
                .pick(&selector)
                .and_then(|(_, version)| Ok((version, self.reveal(version, decrypt)?)))
            {
                Ok((version, value)) => found.push((selector, version, value)),
                Err(_) => missing.push(text),
            }
        }
        reply::<ManyResult>(|o| {
            o.put_list(ManyResult::PARAMETERS, |l| {
                for (selector, version, value) in &found {
                    l.push(|p| shown(p, call, selector.name, version, selector.qualifier, value));
                }
            });
            o.put_list(ManyResult::INVALID_PARAMETERS, |l| {
                for text in &missing {
                    l.push(text);
                }
            });
        })
    }

    pub(crate) fn get_parameter_history(&mut self, call: &Call) -> Reply {
        let v = request::<HistoryRequest>(call)?;
        let name = v.get(HistoryRequest::NAME).unwrap_or_default();
        let decrypt = v.get(HistoryRequest::WITH_DECRYPTION).unwrap_or(false);
        let limit = v
            .get(HistoryRequest::MAX_RESULTS)
            .map_or(HISTORY_PAGE, |n| usize::try_from(n).unwrap_or(HISTORY_PAGE));
        let after = match v.get(HistoryRequest::NEXT_TOKEN) {
            Some(token) => page::resume(token)?.parse::<i64>().map_err(|_| {
                ApiError::modeled("InvalidNextToken", "The specified token is not valid.")
            })?,
            None => 0,
        };
        let p = self.params.get(name).ok_or_else(not_found)?;
        let mut rows: Vec<(&Version, String)> = Vec::new();
        for version in p.versions().filter(|x| x.number > after).take(limit + 1) {
            rows.push((version, self.reveal(version, decrypt)?));
        }
        let more = rows.len() > limit;
        rows.truncate(limit);
        let token = more
            .then(|| rows.last().map(|(x, _)| page::token(&x.number.to_string())))
            .flatten();
        reply::<History>(|o| {
            o.put_list(History::PARAMETERS, |l| {
                for (version, value) in &rows {
                    l.push(|h| past(h, name, version, value));
                }
            });
            if let Some(token) = &token {
                o.put(History::NEXT_TOKEN, token);
            }
        })
    }
}

/// A parameter as `GetParameter` shows it.
pub(crate) fn shown(
    p: &mut Object<'_, Shown>,
    call: &Call,
    name: &str,
    v: &Version,
    qualifier: Option<&str>,
    value: &str,
) {
    p.put(Shown::NAME, name);
    p.put(Shown::TYPE, v.kind.as_str());
    p.put(Shown::VALUE, value);
    p.put_int(Shown::VERSION, v.number);
    if let Some(q) = qualifier {
        p.put(Shown::SELECTOR, &format!(":{q}"));
    }
    p.put_time(Shown::LAST_MODIFIED_DATE, seconds(v.modified_ms));
    p.put(
        Shown::ARN,
        &name::arn(&call.caller.region, &call.caller.account_id, name),
    );
    p.put(Shown::DATA_TYPE, &v.data_type);
}

fn past(h: &mut Object<'_, Past>, name: &str, v: &Version, value: &str) {
    h.put(Past::NAME, name);
    h.put(Past::TYPE, v.kind.as_str());
    if let Some(k) = &v.key_id {
        h.put(Past::KEY_ID, k);
    }
    h.put_time(Past::LAST_MODIFIED_DATE, seconds(v.modified_ms));
    h.put(Past::LAST_MODIFIED_USER, &v.modified_by);
    if let Some(d) = &v.description {
        h.put(Past::DESCRIPTION, d);
    }
    h.put(Past::VALUE, value);
    if let Some(a) = &v.allowed_pattern {
        h.put(Past::ALLOWED_PATTERN, a);
    }
    h.put_int(Past::VERSION, v.number);
    h.put_list(Past::LABELS, |l| {
        for label in &v.labels {
            l.push(label);
        }
    });
    h.put(Past::TIER, if v.advanced { "Advanced" } else { "Standard" });
    h.put_list(Past::POLICIES, |l| {
        for pol in &v.policies {
            l.push(|i: &mut Object<'_, Inline>| policy_of(i, pol));
        }
    });
    h.put(Past::DATA_TYPE, &v.data_type);
}

pub(crate) fn policy_of(i: &mut Object<'_, Inline>, pol: &crate::model::Policy) {
    i.put(Inline::POLICY_TEXT, &pol.text);
    i.put(Inline::POLICY_TYPE, &pol.kind);
    i.put(Inline::POLICY_STATUS, "Pending");
}

#[allow(clippy::cast_precision_loss)]
pub(crate) fn seconds(ms: i64) -> f64 {
    ms as f64 / 1000.0
}

use crate::name;
