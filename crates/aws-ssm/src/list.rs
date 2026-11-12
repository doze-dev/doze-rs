//! Listing: `GetParametersByPath` and `DescribeParameters`, with the filters both take.

use aws_model_ssm::keys::{
    DescribeParametersRequest as Describe, DescribeParametersResult as Described,
    GetParametersByPathRequest as ByPath, GetParametersByPathResult as ByPathResult,
    ParameterMetadata as Meta, ParameterStringFilter as Filter, ParametersFilter as Legacy,
};
use aws_schema::build::Object;
use aws_schema::view::View;
use aws_wire::ApiError;
use aws_wire::service::{Call, Reply};

use crate::core::Core;
use crate::model::Parameter;
use crate::read::{policy_of, seconds, shown};
use crate::{name, page};
use aws_wire::handler::{reply, request};

/// Parameters with their names.
type Rows<'a> = Vec<(&'a str, &'a Parameter)>;

const BY_PATH_PAGE: usize = 10;
const DESCRIBE_PAGE: usize = 10;

/// One filter: a key, how to compare, and what to compare with.
struct Cond {
    key: String,
    option: Option<String>,
    values: Vec<String>,
}

fn invalid(code: &'static str, message: String) -> ApiError {
    ApiError::modeled(code, message)
}

impl Cond {
    fn holds(&self, name: &str, p: &Parameter) -> Result<bool, ApiError> {
        let v = &p.latest;
        let option = self.option.as_deref().unwrap_or("Equals");
        let text = |actual: &str| -> Result<bool, ApiError> {
            Ok(self.values.iter().any(|want| match option {
                "Equals" => actual == want,
                "BeginsWith" => actual.starts_with(want.as_str()),
                _ => false,
            }))
        };
        if !matches!(
            option,
            "Equals" | "BeginsWith" | "Contains" | "Recursive" | "OneLevel"
        ) {
            return Err(invalid(
                "InvalidFilterOption",
                format!("The filter option {option} is not valid."),
            ));
        }
        match self.key.as_str() {
            "Name" if option == "Contains" => {
                Ok(self.values.iter().any(|w| name.contains(w.as_str())))
            }
            "Name" => Ok(text(name)? || text(name.trim_start_matches('/'))?),
            "Type" => text(v.kind.as_str()),
            "KeyId" => text(v.key_id.as_deref().unwrap_or_default()),
            "Tier" => text(if v.advanced { "Advanced" } else { "Standard" }),
            "DataType" => text(&v.data_type),
            "Label" => Ok(self.values.iter().any(|want| p.labelled(want).is_some())),
            "Path" => Ok(self
                .values
                .iter()
                .any(|path| under(name, path, option == "Recursive"))),
            key => match key.strip_prefix("tag:") {
                Some(tag) => Ok(p
                    .tags
                    .get(tag)
                    .is_some_and(|actual| self.values.iter().any(|w| w == actual))),
                None => Err(invalid(
                    "InvalidFilterKey",
                    format!("The filter key {key} is not valid."),
                )),
            },
        }
    }
}

/// Whether `name` is under `path` (one level down, or any depth).
fn under(name: &str, path: &str, recursive: bool) -> bool {
    let path = path.trim_end_matches('/');
    let Some(rest) = name.strip_prefix(path).and_then(|r| r.strip_prefix('/')) else {
        return false;
    };
    !rest.is_empty() && (recursive || !rest.contains('/'))
}

fn conds<'a>(filters: impl Iterator<Item = View<'a, Filter>>) -> Vec<Cond> {
    filters
        .map(|f| Cond {
            key: f.get(Filter::KEY).unwrap_or_default().to_owned(),
            option: f.get(Filter::OPTION).map(str::to_owned),
            values: f
                .get(Filter::VALUES)
                .map(|v| v.map(str::to_owned).collect())
                .unwrap_or_default(),
        })
        .collect()
}

impl Core {
    /// The names, in order, after `after`, that pass `keep`; at most `limit`, and whether more follow.
    fn page_of<'a>(
        &'a self,
        rows: impl Iterator<Item = (&'a str, &'a Parameter)>,
        after: Option<&str>,
        limit: usize,
        keep: &dyn Fn(&str, &Parameter) -> Result<bool, ApiError>,
    ) -> Result<(Rows<'a>, bool), ApiError> {
        let mut out = Vec::new();
        for (name, p) in rows.filter(|(n, _)| after.is_none_or(|a| *n > a)) {
            if keep(name, p)? {
                out.push((name, p));
                if out.len() > limit {
                    break;
                }
            }
        }
        let more = out.len() > limit;
        out.truncate(limit);
        Ok((out, more))
    }

    pub(crate) async fn get_parameters_by_path(&mut self, call: &Call) -> Reply {
        let v = request::<ByPath>(call)?;
        let path = name::check_path(v.get(ByPath::PATH).unwrap_or_default())?;
        let recursive = v.get(ByPath::RECURSIVE).unwrap_or(false);
        let decrypt = v.get(ByPath::WITH_DECRYPTION).unwrap_or(false);
        let limit = v
            .get(ByPath::MAX_RESULTS)
            .map_or(BY_PATH_PAGE, |n| usize::try_from(n).unwrap_or(BY_PATH_PAGE));
        let after = v.get(ByPath::NEXT_TOKEN).map(page::resume).transpose()?;
        let filters = conds(v.get(ByPath::PARAMETER_FILTERS).into_iter().flatten());
        let prefix = format!("{path}/");
        let keep = |name: &str, p: &Parameter| {
            if !under(name, path, recursive) {
                return Ok(false);
            }
            for f in &filters {
                if !matches!(f.key.as_str(), "Type" | "KeyId" | "Label") {
                    return Err(invalid(
                        "InvalidFilterKey",
                        format!("The filter key {} is not valid.", f.key),
                    ));
                }
                if !f.holds(name, p)? {
                    return Ok(false);
                }
            }
            Ok(true)
        };
        let (rows, more) = self.page_of(
            self.params.with_prefix(&prefix),
            after.as_deref(),
            limit,
            &keep,
        )?;
        let mut shown_rows = Vec::new();
        for (name, p) in &rows {
            shown_rows.push((
                *name,
                &p.latest,
                self.reveal(call, name, &p.latest, decrypt).await?,
            ));
        }
        let token = more
            .then(|| rows.last().map(|(n, _)| page::token(n)))
            .flatten();
        reply::<ByPathResult>(|o| {
            o.put_list(ByPathResult::PARAMETERS, |l| {
                for (name, version, value) in &shown_rows {
                    l.push(|p| shown(p, call, name, version, None, value));
                }
            });
            if let Some(token) = &token {
                o.put(ByPathResult::NEXT_TOKEN, token);
            }
        })
    }

    pub(crate) fn describe_parameters(&mut self, call: &Call) -> Reply {
        let v = request::<Describe>(call)?;
        let limit = v.get(Describe::MAX_RESULTS).map_or(DESCRIBE_PAGE, |n| {
            usize::try_from(n).unwrap_or(DESCRIBE_PAGE)
        });
        let after = v.get(Describe::NEXT_TOKEN).map(page::resume).transpose()?;
        let mut all = conds(v.get(Describe::PARAMETER_FILTERS).into_iter().flatten());
        for f in v.get(Describe::FILTERS).into_iter().flatten() {
            all.push(Cond {
                key: f.get(Legacy::KEY).unwrap_or_default().to_owned(),
                option: None,
                values: f
                    .get(Legacy::VALUES)
                    .map(|v| v.map(str::to_owned).collect())
                    .unwrap_or_default(),
            });
        }
        let keep = |name: &str, p: &Parameter| {
            for f in &all {
                if !f.holds(name, p)? {
                    return Ok(false);
                }
            }
            Ok(true)
        };
        let (rows, more) = self.page_of(self.params.iter(), after.as_deref(), limit, &keep)?;
        let token = more
            .then(|| rows.last().map(|(n, _)| page::token(n)))
            .flatten();
        reply::<Described>(|o| {
            o.put_list(Described::PARAMETERS, |l| {
                for (name, p) in &rows {
                    l.push(|m| described(m, call, name, p));
                }
            });
            if let Some(token) = &token {
                o.put(Described::NEXT_TOKEN, token);
            }
        })
    }
}

fn described(m: &mut Object<'_, Meta>, call: &Call, name: &str, p: &Parameter) {
    let v = &p.latest;
    m.put(Meta::NAME, name);
    m.put(
        Meta::ARN,
        &name::arn(&call.caller.region, &call.caller.account_id, name),
    );
    m.put(Meta::TYPE, v.kind.as_str());
    if let Some(k) = &v.key_id {
        m.put(Meta::KEY_ID, k);
    }
    m.put_time(Meta::LAST_MODIFIED_DATE, seconds(v.modified_ms));
    m.put(Meta::LAST_MODIFIED_USER, &v.modified_by);
    if let Some(d) = &v.description {
        m.put(Meta::DESCRIPTION, d);
    }
    if let Some(a) = &v.allowed_pattern {
        m.put(Meta::ALLOWED_PATTERN, a);
    }
    m.put_int(Meta::VERSION, v.number);
    m.put(Meta::TIER, if v.advanced { "Advanced" } else { "Standard" });
    m.put_list(Meta::POLICIES, |l| {
        for pol in &v.policies {
            l.push(|i| policy_of(i, pol));
        }
    });
    m.put(Meta::DATA_TYPE, &v.data_type);
}
