//! A large Fetch response written both ways the crate can: into one buffer (`response::encode`, every record byte copied
//! into it) and as a frame of pieces (`response::frame`, the records left in the buffers the log read them into, written
//! with a vectored write). For each shape it prints what the encode allocated and still holds when the write begins, the
//! time to encode, and the time to encode and write the frame to a loopback socket that a thread drains (until the reader
//! has every byte), as the median of several rounds.
//!
//! Run: `cargo run --release -p kafka-wire --example fetch_frame`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::print_stdout,
    clippy::cast_precision_loss
)]

use std::alloc::System;
use std::io::{IoSlice, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use bytes::{Buf, Bytes};
use kafka_protocol::messages::FetchResponse;
use kafka_protocol::messages::fetch_response::{FetchableTopicResponse, PartitionData};
use kafka_wire::StrBytes;
use kafka_wire::api::Response;
use kafka_wire::messages::TopicName;
use kafka_wire::response::{self, Frame};
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

const ROUNDS: usize = 9;
/// Fetch v12: the flexible form most clients use today.
const VERSION: i16 = 12;

/// A Fetch response of `partitions` partitions sharing `total` bytes of records, each partition's its own buffer.
fn fetch(total: usize, partitions: usize) -> Response {
    let each = total / partitions;
    let mut t = FetchableTopicResponse::default();
    t.topic = TopicName(StrBytes::from_static_str("big"));
    t.partitions = (0..partitions)
        .map(|i| {
            let mut p = PartitionData::default();
            p.partition_index = i32::try_from(i).unwrap();
            p.records = Some(Bytes::from(vec![7_u8; each]));
            p
        })
        .collect();
    let mut r = FetchResponse::default();
    r.responses = vec![t];
    Response::Fetch(r)
}

/// A reader on the other end of a loopback socket: it drains `len` bytes each time it is told to, and says when it has.
struct Drain {
    writer: TcpStream,
    ask: mpsc::Sender<usize>,
    done: mpsc::Receiver<()>,
}

impl Drain {
    fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let writer = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (mut reader, _) = listener.accept().unwrap();
        let (ask, asked) = mpsc::channel::<usize>();
        let (said, done) = mpsc::channel();
        std::thread::spawn(move || {
            let mut buf = vec![0_u8; 1 << 20];
            for mut left in asked {
                while left > 0 {
                    let n = reader.read(&mut buf).unwrap();
                    assert!(n > 0, "the writer closed early");
                    left -= n;
                }
                said.send(()).unwrap();
            }
        });
        Self { writer, ask, done }
    }

    /// Writes `frame` (vectored) and waits for the reader to have all of it.
    fn send(&mut self, mut frame: impl Buf) {
        self.ask.send(frame.remaining()).unwrap();
        while frame.has_remaining() {
            let mut slots = [IoSlice::new(&[]); 64];
            let n = frame.chunks_vectored(&mut slots);
            let wrote = self.writer.write_vectored(&slots[..n]).unwrap();
            frame.advance(wrote);
        }
        self.done.recv().unwrap();
    }
}

struct Run {
    allocated: usize,
    allocations: usize,
    held: isize,
    encode: Duration,
    total: Duration,
}

fn median(mut v: Vec<Duration>) -> Duration {
    v.sort();
    v[v.len() / 2]
}

fn measure(drain: &mut Drain, response: &Response, shared: bool) -> Run {
    let mut encodes = Vec::new();
    let mut totals = Vec::new();
    let mut last = None;
    for _ in 0..ROUNDS {
        let region = Region::new(GLOBAL);
        let began = Instant::now();
        let frame = if shared {
            response::frame(1, VERSION, response).unwrap()
        } else {
            Frame::from(response::encode(1, VERSION, response).unwrap())
        };
        let encoded = began.elapsed();
        let stats = region.change();
        drain.send(frame);
        totals.push(began.elapsed());
        encodes.push(encoded);
        last = Some(stats);
    }
    let stats = last.unwrap();
    Run {
        allocated: stats.bytes_allocated + stats.bytes_reallocated.max(0).unsigned_abs(),
        allocations: stats.allocations + stats.reallocations,
        held: isize::try_from(stats.bytes_allocated).unwrap()
            - isize::try_from(stats.bytes_deallocated).unwrap(),
        encode: median(encodes),
        total: median(totals),
    }
}

fn main() {
    let mut drain = Drain::new();
    println!(
        "{:<28} {:<7} {:>12} {:>7} {:>12} {:>11} {:>14}",
        "Fetch v12", "path", "allocated B", "allocs", "held B", "encode", "encode+write"
    );
    for (label, total, partitions) in [
        ("1 MiB, 1 partition", 1 << 20, 1),
        ("50 MiB, 1 partition", 50 << 20, 1),
        ("50 MiB, 16 partitions", 50 << 20, 16),
    ] {
        let response = fetch(total, partitions);
        for (path, shared) in [("copy", false), ("shared", true)] {
            let r = measure(&mut drain, &response, shared);
            println!(
                "{label:<28} {path:<7} {:>12} {:>7} {:>12} {:>9.3}ms {:>12.3}ms",
                r.allocated,
                r.allocations,
                r.held,
                r.encode.as_secs_f64() * 1e3,
                r.total.as_secs_f64() * 1e3,
            );
        }
    }
}
