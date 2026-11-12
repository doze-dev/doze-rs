//! The binary as a process: it listens, serves a client over a socket, stops on SIGTERM having flushed, and comes back
//! with its data; and a second one on the same data directory is refused.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]
#![cfg(unix)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bytes::{Buf, Bytes};
use kafka_protocol::messages::create_topics_request::CreatableTopic;
use kafka_protocol::messages::fetch_request::{FetchPartition, FetchTopic};
use kafka_protocol::messages::produce_request::{PartitionProduceData, TopicProduceData};
use kafka_protocol::messages::{
    ApiKey, CreateTopicsRequest, CreateTopicsResponse, FetchRequest, FetchResponse, ProduceRequest,
    ProduceResponse, ResponseHeader, TopicName,
};
use kafka_protocol::protocol::{Decodable, Encodable, StrBytes};
use kafka_wire::batch::build::BatchBuilder;
use kafka_wire::testing::{framed, request_payload};

/// How long a test waits for a process that is expected to exit by itself. A mutated binary that hangs fails the test in
/// this long, not after the test harness's own minutes.
const PROCESS_WAIT: Duration = Duration::from_secs(20);

/// `Command::output` with a limit: the process is killed, and the test fails with what it had said, if it is still
/// running after `limit`.
trait OutputWithin {
    fn output_within(&mut self, limit: Duration) -> std::process::Output;
}

/// Reads a pipe to its end on a thread, so that a process that fills one is never blocked on it.
fn drain<R: Read + Send + 'static>(mut pipe: R) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = pipe.read_to_end(&mut bytes);
        bytes
    })
}

impl OutputWithin for Command {
    fn output_within(&mut self, limit: Duration) -> std::process::Output {
        let mut child = self
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = drain(child.stdout.take().unwrap());
        let stderr = drain(child.stderr.take().unwrap());
        let deadline = Instant::now() + limit;
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                let said = String::from_utf8_lossy(&stderr.join().unwrap_or_default()).into_owned();
                panic!("the process was still running after {limit:?}; stderr so far: {said}");
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        std::process::Output {
            status,
            stdout: stdout.join().unwrap(),
            stderr: stderr.join().unwrap(),
        }
    }
}

struct Broker {
    child: Child,
    port: u16,
    /// Everything the broker has written to stderr so far.
    log: Arc<Mutex<String>>,
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("doze-process-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// Starts the binary on a free port and waits until it says it is listening.
fn start(data: &Path) -> Broker {
    start_with(data, &[])
}

/// [`start`], with more flags.
fn start_with(data: &Path, flags: &[&str]) -> Broker {
    start_with_env(data, flags, &[])
}

/// [`start_with`], with environment variables set for the process.
fn start_with_env(data: &Path, flags: &[&str], env: &[(&str, &str)]) -> Broker {
    let mut child = Command::new(env!("CARGO_BIN_EXE_doze-kafka"))
        .args(["--listen", "127.0.0.1:0", "--data-dir"])
        .arg(data)
        // Every test broker has a console of its own on a free port, unless the test chose an address.
        .args(
            ["--console-addr", "127.0.0.1:0"]
                .iter()
                .filter(|_| !flags.contains(&"--console-addr")),
        )
        .args(flags)
        .envs(env.iter().copied())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stderr = child.stderr.take().unwrap();
    let mut lines = BufReader::new(stderr).lines();
    let deadline = Instant::now() + Duration::from_secs(20);
    let log = Arc::new(Mutex::new(String::new()));
    while Instant::now() < deadline {
        let Some(Ok(line)) = lines.next() else { break };
        log.lock().unwrap().push_str(&format!("{line}\n"));
        if let Some(at) = line.find("addr=") {
            let addr = &line[at + 5..];
            let port = addr
                .split_whitespace()
                .next()
                .unwrap()
                .rsplit(':')
                .next()
                .unwrap();
            // Keep reading so the broker never blocks on a full pipe.
            let rest = Arc::clone(&log);
            std::thread::spawn(move || {
                for line in lines.map_while(Result::ok) {
                    rest.lock().unwrap().push_str(&format!("{line}\n"));
                }
            });
            return Broker {
                child,
                port: port.parse().unwrap(),
                log,
            };
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    panic!("the broker did not say it was listening");
}

impl Broker {
    fn connect(&self) -> TcpStream {
        let s = TcpStream::connect(("127.0.0.1", self.port)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        s
    }

    fn terminate(&mut self) -> std::process::ExitStatus {
        let ok = Command::new("kill")
            .arg("-TERM")
            .arg(self.child.id().to_string())
            .status()
            .unwrap();
        assert!(ok.success());
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status;
            }
            assert!(
                Instant::now() < deadline,
                "SIGTERM did not stop the broker within ten seconds"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Broker {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn call<S: Read + Write, Q: Encodable, A: Decodable>(
    s: &mut S,
    key: ApiKey,
    version: i16,
    q: &Q,
) -> A {
    let payload = request_payload(key, version, 7, Some("process-test"), q);
    s.write_all(&framed(&payload)).unwrap();
    let mut len = [0; 4];
    s.read_exact(&mut len).unwrap();
    let mut body = vec![0; u32::from_be_bytes(len) as usize];
    s.read_exact(&mut body).unwrap();
    let mut body = Bytes::from(body);
    let header = ResponseHeader::decode(&mut body, key.response_header_version(version)).unwrap();
    assert_eq!(header.correlation_id, 7);
    let answer = A::decode(&mut body, version).unwrap();
    assert!(!body.has_remaining());
    answer
}

fn name(s: &str) -> TopicName {
    TopicName(StrBytes::from_string(s.to_owned()))
}

fn create<S: Read + Write>(s: &mut S, topic: &str) {
    let mut t = CreatableTopic::default();
    t.name = name(topic);
    t.num_partitions = 2;
    t.replication_factor = 1;
    let mut q = CreateTopicsRequest::default();
    q.topics.push(t);
    let a: CreateTopicsResponse = call(s, ApiKey::CreateTopics, 7, &q);
    assert_eq!(a.topics[0].error_code, 0, "{:?}", a.topics[0].error_message);
}

fn batch() -> Bytes {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    BatchBuilder::new().records(3).timestamps(now, now).build()
}

fn produce<S: Read + Write>(s: &mut S, topic: &str, records: Bytes) -> i64 {
    let mut p = PartitionProduceData::default();
    p.index = 1;
    p.records = Some(records);
    let mut t = TopicProduceData::default();
    t.name = name(topic);
    t.partition_data.push(p);
    let mut q = ProduceRequest::default();
    q.acks = -1;
    q.timeout_ms = 5000;
    q.topic_data.push(t);
    let a: ProduceResponse = call(s, ApiKey::Produce, 9, &q);
    let r = &a.responses[0].partition_responses[0];
    assert_eq!(r.error_code, 0);
    r.base_offset
}

fn fetch<S: Read + Write>(s: &mut S, topic: &str) -> Bytes {
    let mut p = FetchPartition::default();
    p.partition = 1;
    p.fetch_offset = 0;
    p.partition_max_bytes = 1 << 20;
    let mut t = FetchTopic::default();
    t.topic = name(topic);
    t.partitions.push(p);
    let mut q = FetchRequest::default();
    q.max_wait_ms = 0;
    q.min_bytes = 1;
    q.max_bytes = 1 << 20;
    q.topics.push(t);
    let a: FetchResponse = call(s, ApiKey::Fetch, 12, &q);
    let r = &a.responses[0].partitions[0];
    assert_eq!(r.error_code, 0);
    r.records.clone().unwrap()
}

#[test]
fn it_serves_a_client_stops_on_sigterm_and_comes_back_with_the_data() {
    let data = scratch("serve");
    let sent = batch();
    let mut first = start(&data);
    let mut s = first.connect();
    create(&mut s, "t");
    assert_eq!(produce(&mut s, "t", sent.clone()), 0);
    assert_eq!(
        &fetch(&mut s, "t")[8..],
        &sent[8..],
        "what was produced is fetched, byte for byte after the offset"
    );
    drop(s);
    assert!(
        first.terminate().success(),
        "a stop on SIGTERM is a success"
    );
    assert!(data.join("clean-shutdown").exists(), "and it is recorded");

    let second = start(&data);
    let mut s = second.connect();
    assert_eq!(
        &fetch(&mut s, "t")[8..],
        &sent[8..],
        "the data is there after a restart"
    );
    assert_eq!(
        produce(&mut s, "t", sent.clone()),
        3,
        "and the next offset carries on"
    );
    let _ = std::fs::remove_dir_all(&data);
}

#[test]
fn an_acknowledged_write_survives_the_process_being_killed() {
    let data = scratch("kill");
    let sent = batch();
    let mut first = start(&data);
    let mut s = first.connect();
    create(&mut s, "t");
    produce(&mut s, "t", sent.clone());
    first.child.kill().unwrap(); // SIGKILL: no flush, no marker
    first.child.wait().unwrap();
    assert!(!data.join("clean-shutdown").exists());
    let second = start(&data);
    let mut s = second.connect();
    assert_eq!(&fetch(&mut s, "t")[8..], &sent[8..]);
    let _ = std::fs::remove_dir_all(&data);
}

#[test]
fn a_second_broker_on_the_same_data_directory_is_refused() {
    let data = scratch("lock");
    let _first = start(&data);
    let output = Command::new(env!("CARGO_BIN_EXE_doze-kafka"))
        .args(["--listen", "127.0.0.1:0", "--data-dir"])
        .arg(&data)
        .output_within(PROCESS_WAIT);
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("another doze-kafka"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = std::fs::remove_dir_all(&data);
}

#[path = "process/acl.rs"]
mod acl;
#[path = "process/console.rs"]
mod console;
#[path = "process/console_start.rs"]
mod console_start;
#[path = "process/sasl.rs"]
mod sasl;
#[cfg(feature = "tls")]
#[path = "process/tls.rs"]
mod tls;
#[path = "process/users.rs"]
mod users;
