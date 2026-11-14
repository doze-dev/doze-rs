//! What the app shows of a running broker and can do to it, as the Go module had it: topics with their partitions and
//! messages (a rate the host works out from the running total), consumer groups with their lag, a tail of the
//! selected topic, and one action, produce a message. The web console is doze-kafka's own, on the `console` listener;
//! the app can open it (`web_url`), which it could not with the Go module.

use std::collections::BTreeMap;
use std::time::Duration;

use doze_module::caps::{Admin, Console, Rows};
use doze_module::proto::msg::console::{Action, Column, ConsoleSpec, Panel, Resource, Row};
use doze_module::proto::msg::error::{Error, ErrorKind};
use doze_module::{Cx, Result};

use super::client::{Client, Fetched, TopicInfo};
use super::{Kafka, KafkaSpec};

/// How many messages of history a tail starts with, so that a quiet topic still shows what it is for (Go's).
const TAIL_BACK: i64 = 10;
/// How much of a message's value is sent to the console (Go's).
const MAX_VALUE: usize = 240;

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// The topics, then the consumer groups.
pub(crate) async fn resources(c: &mut Client) -> Result<Vec<Resource>> {
    let topics: Vec<TopicInfo> = c.topics().await?.into_values().collect();
    let ends = c.offsets(&topics, -1).await?;
    let mut out = Vec::new();
    for t in &topics {
        let parts: Vec<i64> = t
            .partitions
            .iter()
            .map(|p| ends.get(&(t.name.clone(), *p)).copied().unwrap_or(0))
            .collect();
        let high: i64 = parts.iter().sum();
        let n = t.partitions.len();
        out.push(Resource {
            kind: "topic".into(),
            name: t.name.clone(),
            status: format!("{n} partition{} · high-water {high}", plural(n)),
            info: BTreeMap::from([
                ("partitions".into(), n.to_string()),
                ("high".into(), high.to_string()),
                (
                    "parts".into(),
                    parts
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(","),
                ),
            ]),
        });
    }
    for g in c.groups().await? {
        let committed = c.committed(&g.name).await?;
        let lag: i64 = committed
            .iter()
            .map(|(at, done)| (ends.get(at).copied().unwrap_or(*done) - done).max(0))
            .sum();
        let m = g.members;
        out.push(Resource {
            kind: "group".into(),
            name: g.name.clone(),
            status: format!("lag {lag} · {m} member{} · {}", plural(m), g.state),
            info: BTreeMap::from([
                ("members".into(), m.to_string()),
                ("state".into(), g.state.clone()),
                ("lag".into(), lag.to_string()),
            ]),
        });
    }
    Ok(out)
}

/// `HH:MM:SS` (UTC) of a timestamp in milliseconds.
pub(crate) fn clock(ms: i64) -> String {
    let s = ms.div_euclid(1000).rem_euclid(86_400);
    format!("{:02}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

/// A record as a row of the tail: its value cut to `MAX_VALUE` bytes (at a character boundary).
pub(crate) fn row(r: &Fetched) -> Row {
    let mut value = String::from_utf8_lossy(&r.value).into_owned();
    if value.len() > MAX_VALUE {
        let mut cut = MAX_VALUE;
        while !value.is_char_boundary(cut) {
            cut -= 1;
        }
        value.truncate(cut);
        value.push('…');
    }
    Row {
        cells: BTreeMap::from([
            ("time".into(), clock(r.timestamp)),
            ("partition".into(), r.partition.to_string()),
            ("offset".into(), r.offset.to_string()),
            ("key".into(), String::from_utf8_lossy(&r.key).into_owned()),
            ("value".into(), value),
        ]),
        tone: String::new(),
    }
}

/// Streams the selected topic: the last `TAIL_BACK` messages of each partition, then each new one, until the host stops
/// reading. With no topic selected it waits to be restarted with one.
async fn tail(cx: &Cx<KafkaSpec>, panel: &str, topic: &str, out: &Rows<'_>) -> Result<()> {
    if panel != "tail" || topic.is_empty() {
        cx.cancel().cancelled().await;
        return Ok(());
    }
    let mut c = Client::connect(&cx.instance).await?;
    let all = c.topics().await?;
    let Some(t) = all.get(topic).cloned() else {
        return Err(Error::new(
            ErrorKind::NotFound,
            format!("there is no topic {topic}"),
        ));
    };
    let ends = c.offsets(std::slice::from_ref(&t), -1).await?;
    let starts = c.offsets(std::slice::from_ref(&t), -2).await?;
    let mut from: BTreeMap<i32, i64> = t
        .partitions
        .iter()
        .map(|p| {
            let at = (topic.to_string(), *p);
            let end = ends.get(&at).copied().unwrap_or(0);
            let start = starts.get(&at).copied().unwrap_or(0);
            (*p, (end - TAIL_BACK).max(start))
        })
        .collect();
    while !cx.cancel().is_cancelled() {
        let records = tokio::select! {
            r = c.fetch(topic, &mut from) => r,
            () = cx.cancel().cancelled() => return Ok(()),
        };
        match records {
            Ok(records) => {
                for r in &records {
                    out.send(&row(r)).await?;
                }
            }
            Err(e) => {
                let mut bad = Row {
                    cells: BTreeMap::from([("value".into(), e.message.clone())]),
                    tone: "bad".into(),
                };
                bad.cells.insert("time".into(), String::new());
                out.send(&bad).await?;
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
    Ok(())
}

/// Puts `input` on a topic: the one action, which the app offers on each topic row.
async fn produce(c: &mut Client, topic: &str, input: Option<&str>) -> Result<String> {
    let partition = 0;
    let offset = c
        .produce(topic, partition, input.unwrap_or_default().as_bytes())
        .await?;
    Ok(format!(
        "produced to {topic}, partition {partition} offset {offset}"
    ))
}

impl Admin for Kafka {
    async fn resources(&self, cx: &Cx<KafkaSpec>) -> Result<Vec<Resource>> {
        resources(&mut Client::connect(&cx.instance).await?).await
    }

    async fn run_action(
        &self,
        cx: &Cx<KafkaSpec>,
        action: &str,
        resource: &str,
        input: Option<&str>,
    ) -> Result<String> {
        if action != "produce" {
            return Err(Error::new(
                ErrorKind::NotFound,
                format!("kafka has no action {action:?}"),
            ));
        }
        produce(&mut Client::connect(&cx.instance).await?, resource, input).await
    }
}

impl Console for Kafka {
    async fn tail(
        &self,
        cx: &Cx<KafkaSpec>,
        panel: &str,
        selector: &str,
        out: &Rows<'_>,
    ) -> Result<()> {
        tail(cx, panel, selector, out).await
    }
}

/// The one action: put a message on a topic, the thing a person reaches for a console to do.
pub(crate) fn actions() -> Vec<Action> {
    vec![Action {
        id: "produce".into(),
        label: "Produce a message".into(),
        kind: "topic".into(),
        destructive: false,
        input_hint: Some("message value".into()),
    }]
}

fn column(key: &str, title: &str, role: &str, right: bool) -> Column {
    Column {
        key: key.into(),
        title: title.into(),
        role: role.into(),
        align: if right { "right" } else { "left" }.into(),
    }
}

/// The console layout: the Go module's three panels, and the web console the app can open.
pub(crate) fn console() -> ConsoleSpec {
    ConsoleSpec {
        panels: vec![
            Panel {
                id: "topics".into(),
                title: "Topics".into(),
                kind: "table".into(),
                source: "topic".into(),
                columns: vec![
                    column("name", "Topic", "", false),
                    column("partitions", "Partitions", "count", true),
                    column("high", "Messages", "count", true),
                    column("high", "Rate", "rate", true),
                ],
                select: None,
            },
            Panel {
                id: "groups".into(),
                title: "Consumer groups".into(),
                kind: "table".into(),
                source: "group".into(),
                columns: vec![
                    column("name", "Group", "", false),
                    column("state", "State", "state", false),
                    column("members", "Members", "count", true),
                    column("lag", "Lag", "lag", true),
                ],
                select: None,
            },
            Panel {
                id: "tail".into(),
                title: "Tail".into(),
                kind: "stream".into(),
                source: String::new(),
                columns: vec![
                    column("time", "Time", "time", false),
                    column("partition", "P", "", true),
                    column("offset", "Offset", "", true),
                    column("key", "Key", "", false),
                    column("value", "Value", "", false),
                ],
                select: Some("topics".into()),
            },
        ],
        tabs: vec![],
        web_url: super::has_console().then(|| "http://{address:console}".into()),
        front: Some("console".into()),
        extend: vec![],
    }
}
