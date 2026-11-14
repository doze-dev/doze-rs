//! Where the Log and Producers pages get their answers: the broker's facts about a partition's log, DescribeProducers' neighbours
//! and the transactions, or, in the tests and the `boards` example, the larder.

use crate::gather;
use crate::model::{LogView, ProducersView, TxnView};
use crate::source::Source;

impl Source {
    /// The Log page of a partition; `None` if there is no such topic.
    pub async fn log(&self, topic: &str, partition: i32) -> Option<LogView> {
        match self {
            Self::Live(l) => gather::log_view(&l.backend, topic, partition).await,
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => crate::fixtures_log::log(topic, partition),
        }
    }

    /// The Producers page of a partition; `None` if there is no such topic.
    pub async fn producers(
        &self,
        topic: &str,
        partition: i32,
        opened: Vec<i64>,
    ) -> Option<ProducersView> {
        match self {
            Self::Live(l) => {
                let mut v = gather::producers_view(&l.backend, topic, partition, opened).await?;
                if let Some(r) = &l.recorder {
                    let ids: Vec<i64> = v.producers.iter().map(|p| p.id).collect();
                    let mut seen = r.seen_as(&ids);
                    for p in &mut v.producers {
                        p.seen_as = seen.remove(&p.id).unwrap_or_default();
                    }
                }
                Some(v)
            }
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => crate::fixtures_log::producers(topic, partition, opened),
        }
    }

    /// The page of one transaction; `None` if the topic or the transaction is not there.
    pub async fn txn(&self, topic: &str, id: &str) -> Option<TxnView> {
        match self {
            Self::Live(l) => gather::txn_view(&l.backend, topic, id).await,
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => crate::fixtures_log::txn_view(topic, id),
        }
    }
}
