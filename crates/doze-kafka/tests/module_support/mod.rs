//! What the module face's tests share (docs/port/phase11.md): the binary started as doze starts it, the oracle's
//! files, and a few Kafka requests to look at the running broker with.
#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;

use bytes::{Buf, Bytes};
use doze_module_proto::msg::console::Resource;
use doze_module_proto::msg::instance::Address;
use doze_module_proto::msg::run::WithToolchain;
use doze_module_test::{Booted, Harness, Stops};
use doze_os::tree::Stopped;
use kafka_protocol::messages::describe_configs_request::DescribeConfigsResource;
use kafka_protocol::messages::metadata_request::MetadataRequestTopic;
use kafka_protocol::messages::offset_commit_request::{
    OffsetCommitRequestPartition, OffsetCommitRequestTopic,
};
use kafka_protocol::messages::{
    ApiKey, DescribeConfigsRequest, DescribeConfigsResponse, GroupId, MetadataRequest,
    MetadataResponse, OffsetCommitRequest, OffsetCommitResponse, ResponseHeader, TopicName,
};
use kafka_protocol::protocol::{Decodable, Encodable, StrBytes};
use kafka_wire::testing::{framed, request_payload};
use serde_json::Value as Json;

pub(crate) const CORPUS: &str = include_str!("../../../../tools/module-oracle/kafka/corpus.jsonl");
pub(crate) const GOLDEN: &str = include_str!("../../../../tools/module-oracle/kafka/golden.jsonl");

/// The binary under the name a module release gives it (`bin/kafka-plugin`), which is how doze starts the module: a
/// link to the built `doze-kafka`, made once per test process next to the other build outputs.
pub(crate) fn plugin() -> PathBuf {
    static PATH: OnceLock<PathBuf> = OnceLock::new();
    PATH.get_or_init(|| {
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("kafka-module-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = if cfg!(windows) {
            "kafka-plugin.exe"
        } else {
            "kafka-plugin"
        };
        let link = dir.join(exe);
        let _ = std::fs::remove_file(&link);
        let built = env!("CARGO_BIN_EXE_doze-kafka");
        if std::fs::hard_link(built, &link).is_err() {
            std::fs::copy(built, &link).unwrap();
        }
        link
    })
    .clone()
}

pub(crate) fn harness() -> Harness {
    Harness {
        program: plugin(),
        engine: "kafka".into(),
        bin_dir: PathBuf::from("/unused"),
        full: "4".into(),
    }
}

/// Whether the broker in this build serves a schema registry (its flag is the Go edition's).
pub(crate) fn broker_has_registry() -> bool {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_doze-kafka"))
        .arg("--help")
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).contains("--schema-registry-addr")
}

// ---------------------------------------------------------------------------------------------------------------------
// The oracle.

pub(crate) fn case_hcl(id: &str) -> String {
    CORPUS
        .lines()
        .map(|l| serde_json::from_str::<Json>(l).unwrap())
        .find(|c| c["id"] == id)
        .unwrap()["hcl"]
        .as_str()
        .unwrap()
        .to_string()
}

// ---------------------------------------------------------------------------------------------------------------------
// On the running broker.

pub(crate) fn backend(b: &Booted) -> String {
    match b.backend() {
        Some(Address::Tcp(a)) => a,
        other => panic!("a TCP backend, got {other:?}"),
    }
}

pub(crate) fn kafka_call<Q: Encodable, A: Decodable>(
    addr: &str,
    key: ApiKey,
    version: i16,
    q: &Q,
) -> A {
    let mut s = TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    let payload = request_payload(key, version, 7, Some("module-test"), q);
    s.write_all(&framed(&payload)).unwrap();
    let mut len = [0; 4];
    s.read_exact(&mut len).unwrap();
    let mut body = vec![0; u32::from_be_bytes(len) as usize];
    s.read_exact(&mut body).unwrap();
    let mut body = Bytes::from(body);
    ResponseHeader::decode(&mut body, key.response_header_version(version)).unwrap();
    let a = A::decode(&mut body, version).unwrap();
    assert!(!body.has_remaining());
    a
}

pub(crate) fn name(s: &str) -> StrBytes {
    StrBytes::from_string(s.to_owned())
}

/// A topic's own configs, as the broker reports them.
pub(crate) fn topic_configs(addr: &str, topic: &str) -> BTreeMap<String, String> {
    let mut r = DescribeConfigsResource::default();
    r.resource_type = 2;
    r.resource_name = name(topic);
    r.configuration_keys = None;
    let mut q = DescribeConfigsRequest::default();
    q.resources.push(r);
    let a: DescribeConfigsResponse = kafka_call(addr, ApiKey::DescribeConfigs, 1, &q);
    a.results[0]
        .configs
        .iter()
        .filter(|c| c.config_source == 1)
        .map(|c| {
            (
                c.name.to_string(),
                c.value.clone().unwrap_or_default().to_string(),
            )
        })
        .collect()
}

/// Sets a topic's own configs to `configs`, as another tool would.
pub(crate) fn set_topic_configs(addr: &str, topic: &str, configs: &BTreeMap<String, String>) {
    use kafka_protocol::messages::alter_configs_request::{AlterConfigsResource, AlterableConfig};
    let mut r = AlterConfigsResource::default();
    r.resource_type = 2;
    r.resource_name = name(topic);
    for (k, v) in configs {
        let mut c = AlterableConfig::default();
        c.name = name(k);
        c.value = Some(name(v));
        r.configs.push(c);
    }
    let mut q = kafka_protocol::messages::AlterConfigsRequest::default();
    q.resources.push(r);
    let a: kafka_protocol::messages::AlterConfigsResponse =
        kafka_call(addr, ApiKey::AlterConfigs, 0, &q);
    assert_eq!(a.responses[0].error_code, 0);
}

/// Asks for a topic by name, letting the broker create it if it auto-creates; the error code.
pub(crate) fn ask_for(addr: &str, topic: &str) -> i16 {
    let mut t = MetadataRequestTopic::default();
    t.name = Some(TopicName(name(topic)));
    let mut q = MetadataRequest::default();
    q.topics = Some(vec![t]);
    q.allow_auto_topic_creation = true;
    let a: MetadataResponse = kafka_call(addr, ApiKey::Metadata, 4, &q);
    a.topics[0].error_code
}

/// Commits `offset` for a group that has no members (as a tool that sets offsets does).
pub(crate) fn commit(addr: &str, group: &str, topic: &str, offset: i64) {
    let mut p = OffsetCommitRequestPartition::default();
    p.partition_index = 0;
    p.committed_offset = offset;
    let mut t = OffsetCommitRequestTopic::default();
    t.name = TopicName(name(topic));
    t.partitions.push(p);
    let mut q = OffsetCommitRequest::default();
    q.group_id = GroupId(name(group));
    q.generation_id_or_member_epoch = -1;
    q.topics.push(t);
    let a: OffsetCommitResponse = kafka_call(addr, ApiKey::OffsetCommit, 2, &q);
    assert_eq!(a.topics[0].partitions[0].error_code, 0);
}

pub(crate) async fn resources(b: &Booted) -> Vec<Resource> {
    b.module.resources(&b.instance).await.unwrap()
}

pub(crate) fn find<'a>(rs: &'a [Resource], kind: &str, name: &str) -> &'a Resource {
    rs.iter()
        .find(|r| r.kind == kind && r.name == name)
        .unwrap_or_else(|| panic!("no {kind} {name} in {rs:#?}"))
}

pub(crate) fn graceful(stops: &Stops) {
    assert!(
        !stops.is_empty()
            && stops
                .iter()
                .all(|(_, r)| matches!(r, Ok(Stopped::Graceful(st)) if st.success())),
        "{stops:?}"
    );
}

pub(crate) fn with(b: &Booted) -> WithToolchain {
    WithToolchain {
        instance: b.instance.clone(),
        toolchain: b.toolchain.clone(),
    }
}
