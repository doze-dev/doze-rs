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
                gather::topics(&l.backend, filter, internal).await
            }
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => crate::fixtures_topics::topics(filter, internal, hint),
        }
    }

    /// A topic's Messages for `query`; `None` if there is no such topic.
    pub async fn messages(&self, name: &str, query: MessageQuery, notes: Vec<String>) -> Option<MessagesView> {
        match self {
            Self::Live(l) => gather::messages(&l.backend, name, query, notes).await,
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => crate::fixtures_topics::messages(name, query, notes),
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
