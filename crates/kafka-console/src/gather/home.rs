//! Home's data: what the broker holds, the state that follows, and the instance card.

use kafka_broker::{Instance, LastStop};
use kafka_wire::api::Response;

use crate::backend::{Backend, requests};
use crate::facts::Facts;
use crate::format::date_time;
use crate::model::{Access, Attention, HomeState, InstanceView, StopView};

/// The user topics and their partitions, from Metadata (internal topics are not the user's).
pub async fn counts(b: &Backend) -> (usize, usize) {
    let Ok(Response::Metadata(m)) = b.call(requests::metadata_all()).await else {
        return (0, 0);
    };
    let user = m.topics.iter().filter(|t| !t.is_internal);
    let (topics, partitions) =
        user.fold((0, 0), |(t, p), topic| (t + 1, p + topic.partitions.len()));
    (topics, partitions)
}

/// The state Home is in: attention when a transaction is close to its timeout, quiet when the broker holds no topics,
/// steady otherwise.
#[must_use]
pub fn choose(topics: usize, attention: Option<Attention>) -> HomeState {
    match attention {
        Some(a) => HomeState::Attention(a),
        None if topics == 0 => HomeState::Quiet,
        None => HomeState::Steady,
    }
}

/// The instance card from what the broker says of this run.
#[must_use]
pub fn instance_view(i: &Instance, facts: &Facts, access: Access, now_ms: i64) -> InstanceView {
    InstanceView {
        started: date_time(i.started_ms),
        up_secs: (now_ms - i.started_ms).max(0) / 1000,
        last_stop: match i.last_stop {
            LastStop::None => StopView::None,
            LastStop::Clean => StopView::Clean,
            LastStop::Unclean => StopView::Unclean,
        },
        open_partitions: i.open_partitions,
        max_open_partitions: i.max_open_partitions,
        open_files: i.process.open_files,
        resident_bytes: i.process.resident_bytes,
        threads: i.process.threads,
        profile: format!("Kafka {}", facts.kafka_major),
        access,
        data_dir: absolute(&i.data_dir),
    }
}

/// The data directory as an absolute path where the system can say it, else as given.
fn absolute(p: &std::path::Path) -> String {
    std::fs::canonicalize(p)
        .unwrap_or_else(|_| p.to_path_buf())
        .display()
        .to_string()
}

#[cfg(test)]
mod tests;
