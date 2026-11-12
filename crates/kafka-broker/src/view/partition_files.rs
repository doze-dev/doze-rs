//! The files of a topic's partitions: size on disk, last write, and whether a failed flush took one offline.
//!
//! Nothing is opened for this and nothing is kept. A partition that is open answers from its reader's snapshot of
//! segments (and says if a flush failed); one that is not is read from its directory. Both give the same facts.

use kafka_auth::authz::operation;
use kafka_storage::fs::Fs;
use kafka_storage::topic_dirs::partition_dir;
use kafka_storage::{SegmentInfo, segments_on_disk};
use kafka_wire::ids::{PartitionId, TopicName};

use super::ConsoleView;
use crate::Caller;

/// One partition's files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartitionFiles {
    /// Its number.
    pub partition: i32,
    /// Its files are open now (it was used lately).
    pub open: bool,
    /// The bytes of its segments.
    pub bytes: u64,
    /// How many segments it has.
    pub segments: usize,
    /// When its newest segment was last written, in milliseconds since the epoch, where the system says.
    pub last_write_ms: Option<i64>,
    /// Why writes to it are refused: the first failed flush, in the writer's words. `None` while it is online.
    pub offline: Option<String>,
    /// The directory could not be read, so the other facts are not known.
    pub unreadable: bool,
}

fn summed(segments: &[SegmentInfo]) -> (u64, Option<i64>) {
    let bytes = segments.iter().map(|s| s.bytes).sum();
    let newest = segments.iter().filter_map(|s| s.modified_ms).max();
    (bytes, newest)
}

impl<F: Fs> ConsoleView<'_, F> {
    /// The files of each partition of `topic`, in partition order, if `caller` may DESCRIBE the topic and it exists;
    /// `None` otherwise (the two are not told apart, as Kafka does not).
    #[must_use]
    pub fn partition_files(&self, caller: &Caller, topic: &str) -> Option<Vec<PartitionFiles>> {
        let b = self.broker;
        if !b.topic_allows(caller, topic, operation::DESCRIBE) {
            return None;
        }
        let name = TopicName::parse(topic).ok()?;
        let found = b.meta.topic(&name)?;
        let mut out = Vec::new();
        for id in found.partitions.ids() {
            out.push(self.one(&found, id));
        }
        Some(out)
    }

    fn one(&self, topic: &kafka_meta::Topic, id: PartitionId) -> PartitionFiles {
        let b = self.broker;
        let mut files = PartitionFiles {
            partition: id.get(),
            open: false,
            bytes: 0,
            segments: 0,
            last_write_ms: None,
            offline: None,
            unreadable: false,
        };
        if let Some(open) = b.partitions.peek(topic.id, id) {
            let reader = open.reader();
            let segments = reader.segments();
            let (bytes, newest) = summed(&segments);
            files.open = true;
            files.bytes = bytes;
            files.segments = segments.len();
            files.last_write_ms = newest;
            files.offline = reader.offline_cause();
            return files;
        }
        let dir = partition_dir(&b.config.data_dir, &topic.name, id);
        match crate::blocking::run(|| segments_on_disk(&b.fs, &dir)) {
            Ok(segments) => {
                let (bytes, newest) = summed(&segments);
                files.bytes = bytes;
                files.segments = segments.len();
                files.last_write_ms = newest;
            }
            Err(_) => files.unreadable = true,
        }
        files
    }
}
