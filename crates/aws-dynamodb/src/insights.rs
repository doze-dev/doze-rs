//! Contributor Insights: `UpdateContributorInsights`, `DescribeContributorInsights` and `ListContributorInsights`, accepted and stored
//! with the status AWS reports. Nothing is computed: there is no CloudWatch Contributor Insights rule behind a setting here, so no rule
//! is named (SCOPE: accept and flag). Terraform's `aws_dynamodb_contributor_insights` enables, waits for `ENABLED`, and disables.
//!
//! The statuses are AWS's: an update answers the transition (`ENABLING`, `DISABLING`) and the setting is in place at once, so the
//! next describe says `ENABLED` or `DISABLED`. That shape, the default mode and every message here are `(memory)`.

use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};

use aws_wire::ApiError;

use crate::core::{Core, Cx};
use crate::describe::seconds;
use crate::req::{int_of, str_of, table_not_found, validation};

pub(crate) const VERSION: u8 = 1;
/// The mode AWS reports when none was asked for `(memory)`.
const DEFAULT_MODE: &str = "ACCESSED_AND_THROTTLED_KEYS";
const MAX_LIST: usize = 100;

/// One table's (or index's) setting.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Insight {
    pub(crate) enabled: bool,
    pub(crate) mode: String,
    pub(crate) updated_ms: i64,
}

/// `table` or `table/index`: a table name has no `/`.
fn key(table: &str, index: Option<&str>) -> String {
    match index {
        Some(i) => format!("{table}/{i}"),
        None => table.to_owned(),
    }
}

impl Core {
    /// The table and index a request names, both checked.
    fn insight_target(&self, cx: &Cx<'_>) -> Result<(String, Option<String>), ApiError> {
        let name = cx.table_name("TableName")?;
        let t = self
            .tables
            .get(&name)
            .ok_or_else(|| table_not_found(&name))?;
        let index = str_of(&cx.body, "IndexName").map(str::to_owned);
        if let Some(i) = &index
            && t.index(i).is_none()
        {
            return Err(ApiError::modeled(
                "ResourceNotFoundException",
                format!("Requested resource not found: Index: {i} not found"),
            ));
        }
        Ok((name, index))
    }

    pub(crate) fn update_insights(&mut self, cx: &Cx<'_>) -> Result<Json, ApiError> {
        let (table, index) = self.insight_target(cx)?;
        let enable = match str_of(&cx.body, "ContributorInsightsAction") {
            Some("ENABLE") => true,
            Some("DISABLE") => false,
            _ => {
                return Err(validation(
                    "ContributorInsightsAction must be ENABLE or DISABLE",
                ));
            }
        };
        let k = key(&table, index.as_deref());
        let mode = str_of(&cx.body, "ContributorInsightsMode")
            .map(str::to_owned)
            .or_else(|| self.insights.get(&k).map(|i| i.mode.clone()))
            .unwrap_or_else(|| DEFAULT_MODE.to_owned());
        self.insights.insert(
            k,
            Insight {
                enabled: enable,
                mode: mode.clone(),
                updated_ms: cx.now_ms,
            },
        );
        let mut out = json!({
            "TableName": table,
            "ContributorInsightsStatus": if enable { "ENABLING" } else { "DISABLING" },
            "ContributorInsightsMode": mode,
        });
        if let Some(i) = index {
            crate::req::set(&mut out, "IndexName", json!(i));
        }
        Ok(out)
    }

    pub(crate) fn describe_insights(&mut self, cx: &Cx<'_>) -> Result<Json, ApiError> {
        let (table, index) = self.insight_target(cx)?;
        let mut out = json!({"TableName": table});
        if let Some(i) = &index {
            crate::req::set(&mut out, "IndexName", json!(i));
        }
        match self.insights.get(&key(&table, index.as_deref())) {
            Some(i) => {
                crate::req::set(&mut out, "ContributorInsightsStatus", json!(status(i)));
                crate::req::set(&mut out, "ContributorInsightsMode", json!(i.mode));
                crate::req::set(&mut out, "LastUpdateDateTime", json!(seconds(i.updated_ms)));
            }
            None => crate::req::set(&mut out, "ContributorInsightsStatus", json!("DISABLED")),
        }
        Ok(out)
    }

    /// Every table and index with a setting, in name order, or those of one table. `NextToken` is the key to start after.
    pub(crate) fn list_insights(&mut self, cx: &Cx<'_>) -> Result<Json, ApiError> {
        let only = match str_of(&cx.body, "TableName") {
            Some(_) => {
                let name = cx.table_name("TableName")?;
                if self.tables.get(&name).is_none() {
                    return Err(table_not_found(&name));
                }
                Some(name)
            }
            None => None,
        };
        let max = match int_of(&cx.body, "MaxResults") {
            Some(n) if n > 0 => usize::try_from(n).unwrap_or(MAX_LIST).min(MAX_LIST),
            _ => MAX_LIST,
        };
        let after = str_of(&cx.body, "NextToken");
        let mut rows: Vec<(&str, &Insight)> = self
            .insights
            .iter()
            .filter(|(k, _)| {
                only.as_deref().is_none_or(|t| {
                    *k == t || k.strip_prefix(t).is_some_and(|r| r.starts_with('/'))
                })
            })
            .filter(|(k, _)| after.is_none_or(|a| *k > a))
            .collect();
        let more = rows.len() > max;
        rows.truncate(max);
        let summaries: Vec<Json> = rows
            .iter()
            .map(|(k, i)| {
                let (table, index) = k
                    .split_once('/')
                    .map_or((*k, None), |(t, ix)| (t, Some(ix)));
                let mut s = json!({
                    "TableName": table,
                    "ContributorInsightsStatus": status(i),
                    "ContributorInsightsMode": i.mode,
                });
                if let Some(ix) = index {
                    crate::req::set(&mut s, "IndexName", json!(ix));
                }
                s
            })
            .collect();
        let mut out = json!({ "ContributorInsightsSummaries": summaries });
        if more && let Some((k, _)) = rows.last() {
            crate::req::set(&mut out, "NextToken", json!(k));
        }
        Ok(out)
    }

    /// A table that goes takes its settings with it.
    pub(crate) fn drop_insights(&mut self, table: &str) {
        let keys: Vec<String> = self
            .insights
            .iter()
            .map(|(k, _)| k.to_owned())
            .filter(|k| k == table || k.strip_prefix(table).is_some_and(|r| r.starts_with('/')))
            .collect();
        for k in keys {
            self.insights.remove(&k);
        }
    }
}

fn status(i: &Insight) -> &'static str {
    if i.enabled { "ENABLED" } else { "DISABLED" }
}
