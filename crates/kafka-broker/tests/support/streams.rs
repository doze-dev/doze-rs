//! Streams groups (KIP-1071): requests a Kafka Streams word count sends, and the answers.

use kafka_wire::StrBytes;
use kafka_wire::api::{Request, Response};
use kafka_wire::messages::streams_group_heartbeat_request::{KeyValue, Subtopology, TaskIds, TopicInfo, Topology};
use kafka_wire::messages::{
    GroupId, StreamsGroupDescribeRequest, StreamsGroupDescribeResponse, StreamsGroupHeartbeatRequest,
    StreamsGroupHeartbeatResponse, TopicName,
};

use super::Rig;

fn name(s: &str) -> TopicName {
    TopicName(StrBytes::from_string(s.to_owned()))
}

fn info(topic: &str, configs: &[(&str, &str)]) -> TopicInfo {
    let mut t = TopicInfo::default();
    t.name = name(topic);
    t.replication_factor = -1;
    t.topic_configs = configs
        .iter()
        .map(|(k, v)| {
            let mut kv = KeyValue::default();
            kv.key = StrBytes::from_string((*k).to_owned());
            kv.value = StrBytes::from_string((*v).to_owned());
            kv
        })
        .collect();
    t
}

/// The word count Kafka Streams sends for application `app` reading `input`: subtopology 0 re-keys into
/// `<app>-words-repartition`, subtopology 1 counts into a store with `<app>-counts-changelog`.
pub fn word_count(app: &str, input: &str) -> Topology {
    let rep = format!("{app}-words-repartition");
    let mut zero = Subtopology::default();
    zero.subtopology_id = StrBytes::from_static_str("0");
    zero.source_topics = vec![name(input)];
    zero.repartition_sink_topics = vec![name(&rep)];
    let mut one = Subtopology::default();
    one.subtopology_id = StrBytes::from_static_str("1");
    one.repartition_source_topics = vec![info(&rep, &[("cleanup.policy", "delete"), ("retention.ms", "-1")])];
    one.state_changelog_topics = vec![info(&format!("{app}-counts-changelog"), &[("cleanup.policy", "compact")])];
    let mut t = Topology::default();
    t.subtopologies = vec![zero, one];
    t
}

pub fn tasks(list: &[(&str, &[i32])]) -> Vec<TaskIds> {
    list.iter()
        .map(|(s, ps)| {
            let mut t = TaskIds::default();
            t.subtopology_id = StrBytes::from_string((*s).to_owned());
            t.partitions = ps.to_vec();
            t
        })
        .collect()
}

/// A heartbeat at `epoch` that says nothing more.
pub fn heartbeat(group: &str, member: &str, epoch: i32) -> StreamsGroupHeartbeatRequest {
    let mut r = StreamsGroupHeartbeatRequest::default();
    r.group_id = GroupId(StrBytes::from_string(group.to_owned()));
    r.member_id = StrBytes::from_string(member.to_owned());
    r.member_epoch = epoch;
    r.endpoint_information_epoch = -1;
    r
}

/// A join of the word count reading `input`, as Kafka Streams sends it.
pub fn join(group: &str, member: &str, input: &str) -> StreamsGroupHeartbeatRequest {
    let mut r = heartbeat(group, member, 0);
    r.rebalance_timeout_ms = 300_000;
    r.topology = Some(word_count(group, input));
    r.active_tasks = Some(Vec::new());
    r.standby_tasks = Some(Vec::new());
    r.warmup_tasks = Some(Vec::new());
    r.process_id = Some(StrBytes::from_string(format!("process-{member}")));
    r.client_tags = Some(Vec::new());
    r.task_offsets = Some(Vec::new());
    r.task_end_offsets = Some(Vec::new());
    r
}

impl Rig {
    pub async fn streams_heartbeat_at(&self, version: i16, req: StreamsGroupHeartbeatRequest) -> StreamsGroupHeartbeatResponse {
        let Response::StreamsGroupHeartbeat(r) = self.ask(version, Request::StreamsGroupHeartbeat(req.into())).await else {
            panic!("not StreamsGroupHeartbeat")
        };
        *r
    }

    pub async fn streams_heartbeat(&self, req: StreamsGroupHeartbeatRequest) -> StreamsGroupHeartbeatResponse {
        self.streams_heartbeat_at(1, req).await
    }

    pub async fn streams_describe(&self, groups: &[&str]) -> StreamsGroupDescribeResponse {
        let mut req = StreamsGroupDescribeRequest::default();
        req.group_ids = groups.iter().map(|g| GroupId(StrBytes::from_string((*g).to_owned()))).collect();
        let Response::StreamsGroupDescribe(r) = self.ask(1, Request::StreamsGroupDescribe(req)).await else {
            panic!("not StreamsGroupDescribe")
        };
        r
    }

    /// `member` joins the word count of `group` with no initial delay, and keeps on until it runs every task; its last
    /// answer.
    pub async fn streams_settle(&self, group: &str, member: &str, input: &str) -> StreamsGroupHeartbeatResponse {
        let quick = [("streams.initial.rebalance.delay.ms", "0"), ("streams.assignment.interval.ms", "0")];
        assert_eq!(self.group_config(group, &quick).await, 0);
        let mut last = self.streams_heartbeat(join(group, member, input)).await;
        assert_eq!(last.error_code, 0, "{last:?}");
        let mut owned: Vec<TaskIds> = Vec::new();
        for _ in 0..10 {
            if let Some(a) = &last.active_tasks {
                owned = a.iter().map(|t| (t.subtopology_id.to_string(), t.partitions.clone())).map(|(s, p)| { let mut x = TaskIds::default(); x.subtopology_id = StrBytes::from_string(s); x.partitions = p; x }).collect();
            }
            let mut hb = heartbeat(group, member, last.member_epoch);
            hb.active_tasks = Some(owned.clone());
            hb.standby_tasks = Some(Vec::new());
            hb.warmup_tasks = Some(Vec::new());
            last = self.streams_heartbeat(hb).await;
            assert_eq!(last.error_code, 0, "{last:?}");
            if owned.iter().map(|t| t.partitions.len()).sum::<usize>() > 0 && last.status.as_ref().is_some_and(Vec::is_empty) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        last
    }
}

impl Rig {
    /// Every config DescribeConfigs lists for group `group`: its value and source.
    pub async fn describe_group_configs(&self, group: &str) -> std::collections::BTreeMap<String, (String, i8)> {
        let r = self.describe_configs(4, 32, group, None, false, false).await;
        r.configs
            .into_iter()
            .map(|c| (c.name.to_string(), (c.value.map(|v| v.to_string()).unwrap_or_default(), c.config_source)))
            .collect()
    }

    /// The configs DescribeConfigs lists for topic `name`, by name.
    pub async fn topic_configs(&self, name: &str) -> std::collections::BTreeMap<String, String> {
        let r = self.describe_configs(4, 2, name, None, false, false).await;
        r.configs
            .into_iter()
            .map(|c| (c.name.to_string(), c.value.map(|v| v.to_string()).unwrap_or_default()))
            .collect()
    }
}
