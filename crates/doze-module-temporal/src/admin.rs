//! What the app shows of a running temporal: its own Web UI as the console's web tab, and the namespaces with their
//! retention, state and running workflows. The Go module showed nothing.

use crate::caps::cli;
use crate::{Temporal, TemporalSpec};
use doze_module::caps::Admin;
use doze_module::proto::msg::console::{Column, ConsoleSpec, Extension, Node, Panel, Resource};
use doze_module::proto::msg::error::{Error, ErrorKind};
use doze_module::{Cx, Result};
use std::collections::BTreeMap;

/// Retention as a person reads it: `7 days`, `36 hours`, `90 minutes`.
fn readable(ms: Option<u64>) -> String {
    let Some(ms) = ms else { return String::new() };
    let (secs, day, hour) = (ms / 1000, 86_400, 3_600);
    match secs {
        s if s >= day && s % day == 0 => format!("{} days", s / day),
        s if s >= hour && s % hour == 0 => format!("{} hours", s / hour),
        s => format!("{} minutes", s / 60),
    }
}

impl Admin for Temporal {
    /// Every namespace but temporal's own, with its running workflows.
    async fn resources(&self, cx: &Cx<TemporalSpec>) -> Result<Vec<Resource>> {
        let cli = cli(&cx.instance, None)?;
        let mut out = Vec::new();
        for (name, has) in cli
            .list()
            .await?
            .into_iter()
            .filter(|(n, _)| n != "temporal-system")
        {
            let running = cli.running(&name).await?;
            let state = has
                .state
                .trim_start_matches("NAMESPACE_STATE_")
                .to_ascii_lowercase();
            let info = BTreeMap::from([
                ("retention".to_string(), readable(has.retention_ms)),
                ("running".to_string(), running.to_string()),
                ("description".to_string(), has.description),
            ]);
            out.push(Resource {
                kind: "namespace".into(),
                name,
                status: state,
                info,
            });
        }
        Ok(out)
    }

    async fn run_action(
        &self,
        _cx: &Cx<TemporalSpec>,
        action: &str,
        _resource: &str,
        _input: Option<&str>,
    ) -> Result<String> {
        Err(Error::new(
            ErrorKind::NotFound,
            format!("temporal has no action {action:?}: use its Web UI"),
        ))
    }
}

/// The Web UI first (it is temporal's own console), the namespaces table, and the table on the app's overview.
pub(crate) fn console() -> ConsoleSpec {
    let column = |key: &str, title: &str, role: &str, align: &str| Column {
        key: key.into(),
        title: title.into(),
        role: role.into(),
        align: align.into(),
    };
    let namespaces = Panel {
        id: "namespaces".into(),
        title: "Namespaces".into(),
        kind: "table".into(),
        source: "namespace".into(),
        columns: vec![
            column("name", "Namespace", "", "left"),
            column("status", "State", "state", "left"),
            column("retention", "Retention", "", "left"),
            column("running", "Running", "count", "right"),
        ],
        select: None,
    };
    let table = Node {
        kind: "table".into(),
        panel: "namespaces".into(),
        title: "Namespaces".into(),
        weight: 1,
        ..Node::default()
    };
    ConsoleSpec {
        panels: vec![namespaces],
        tabs: vec![],
        web_url: Some("http://{address:ui}".into()),
        front: Some("web".into()),
        extend: vec![Extension {
            after: "vitals".into(),
            node: table,
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retention_reads_in_the_largest_whole_unit() {
        assert_eq!(readable(Some(604_800_000)), "7 days");
        assert_eq!(readable(Some(129_600_000)), "36 hours");
        assert_eq!(readable(Some(5_400_000)), "90 minutes");
        assert_eq!(readable(None), "");
    }
}
