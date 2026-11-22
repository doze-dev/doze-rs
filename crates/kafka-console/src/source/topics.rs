//! What the Topics pages ask of the source: the list, a topic with its Messages, one record, the partitions.

use super::Source;
use crate::gather;
use crate::model::{MessageQuery, MessagesView, RecordView, TopicHead, TopicsView};

impl Source {
    /// The topics whose name contains `filter`. `hint` names a state for the fixtures only (`none`).
    pub async fn topics(&self, filter: &str, internal: bool, hint: Option<&str>) -> TopicsView {
        match self {
            Self::Live(l) => {
                let _ = hint;
                let mut view = gather::topics(&l.backend, filter, internal).await;
                if let Some(rec) = &l.recorder {
                    let names: Vec<String> = view.rows.iter().map(|r| r.name.clone()).collect();
                    let mut by = rec.writers(&names);
                    for r in &mut view.rows {
                        r.written_by = by.remove(&r.name).unwrap_or_default();
                    }
                }
                view
            }
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => crate::fixtures_topics::topics(filter, internal, hint),
        }
    }

    /// The clients the request recorder saw write to `topic`, newest first, at most three (none without a recorder).
    #[must_use]
    pub fn writers(&self, topic: &str) -> Vec<String> {
        match self {
            Self::Live(l) => l.recorder.as_ref().map_or_else(Vec::new, |r| {
                r.writers(&[topic.to_owned()])
                    .remove(topic)
                    .unwrap_or_default()
            }),
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => {
                if topic == "orders" {
                    vec!["orders-svc".to_owned()]
                } else {
                    Vec::new()
                }
            }
        }
    }

    /// A topic's Messages for `query`; `None` if there is no such topic.
    pub async fn messages(
        &self,
        name: &str,
        query: MessageQuery,
        notes: Vec<String>,
    ) -> Option<MessagesView> {
        match self {
            Self::Live(l) => gather::messages(&l.backend, name, query, notes).await,
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => crate::fixtures_topics::messages(name, query, notes),
        }
    }

    /// One look of a Live tail page's stream (`crate::tail`). The fixtures never change: their topics are followed, and
    /// nothing is ever read.
    pub async fn tail_look(&self, f: &mut crate::tail::Follow) -> crate::tail::Looked {
        match self {
            Self::Live(l) => gather::tail_look(&l.backend, f).await,
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => {
                let rows = self.topics("", true, None).await.rows;
                f.topics = rows
                    .into_iter()
                    .filter(|r| f.ask.wants(&r.name, r.internal))
                    .take(crate::tail::MOST_TOPICS)
                    .map(|r| crate::tail::TailTopic {
                        name: r.name,
                        partitions: r.partitions,
                    })
                    .collect();
                f.opened = true;
                crate::tail::Looked::default()
            }
        }
    }

    /// What every page of a topic says about it; `None` if there is no such topic.
    pub async fn topic_head(&self, name: &str) -> Option<TopicHead> {
        match self {
            Self::Live(l) => gather::topic_head(&l.backend, name).await,
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => crate::fixtures_topics::head(name),
        }
    }

    /// One record; `None` if the topic, the partition or the record is not there.
    pub async fn record(&self, name: &str, partition: i32, offset: i64) -> Option<RecordView> {
        match self {
            Self::Live(l) => gather::record(&l.backend, name, partition, offset).await,
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => crate::fixtures_topics::record(name, partition, offset),
        }
    }

    /// The address clients are told, for the commands the pages show.
    #[must_use]
    pub fn listen(&self) -> String {
        match self {
            Self::Live(l) => l.facts.bootstrap(),
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => crate::fixtures::ADDRESS.to_owned(),
        }
    }

    /// The fragments a page asks the stream to watch: none over the fixtures, which do not change.
    #[must_use]
    pub fn watching(&self, ids: &'static [&'static str]) -> &'static [&'static str] {
        match self {
            Self::Live(_) => ids,
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => &[],
        }
    }
}
