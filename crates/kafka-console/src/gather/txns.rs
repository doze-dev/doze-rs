//! The transactions that are open, with what each has written, from ListTransactions and DescribeTransactions.

use kafka_wire::api::Response;

use crate::backend::{Backend, requests};

/// An open transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Open {
    pub id: String,
    pub producer_id: i64,
    pub producer_epoch: i16,
    pub state: String,
    pub since_ms: i64,
    pub timeout_ms: i64,
    /// `(topic, partitions)` it has written to.
    pub topics: Vec<(String, Vec<i32>)>,
}

/// The ongoing transactions (at most the 20 the first page asks about).
pub async fn ongoing(b: &Backend) -> Vec<Open> {
    let Ok(Response::ListTransactions(list)) = b.call(requests::list_ongoing_transactions()).await
    else {
        return Vec::new();
    };
    let ids: Vec<String> = list
        .transaction_states
        .iter()
        .map(|t| t.transactional_id.0.to_string())
        .collect();
    if ids.is_empty() {
        return Vec::new();
    }
    let Ok(Response::DescribeTransactions(d)) = b.call(requests::describe_transactions(&ids)).await
    else {
        return Vec::new();
    };
    d.transaction_states
        .into_iter()
        .filter(|t| t.error_code == 0)
        .map(|t| Open {
            id: t.transactional_id.0.to_string(),
            producer_id: t.producer_id.0,
            producer_epoch: t.producer_epoch,
            state: t.transaction_state.to_string(),
            since_ms: t.transaction_start_time_ms,
            timeout_ms: i64::from(t.transaction_timeout_ms),
            topics: t
                .topics
                .iter()
                .map(|x| (x.topic.0.to_string(), x.partitions.clone()))
                .collect(),
        })
        .collect()
}
