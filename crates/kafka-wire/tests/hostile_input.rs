//! A request must not decide how much memory the broker asks for: an array count the buffer
//! cannot hold may not size an allocation (vendor/kafka-protocol/PATCHES.md, patch 1).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::alloc::System;

use bytes::{BufMut, Bytes, BytesMut};
use kafka_protocol::messages::{JoinGroupRequest, MetadataRequest};
use kafka_protocol::protocol::Decodable;
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

/// The most a few dozen bytes of hostile request may make the decoder allocate: room for 1024
/// elements (the vendored cap) of each array on the path, which is the size of a request frame at most.
const BUDGET: usize = 1 << 20;

fn uvarint(mut v: u64, out: &mut BytesMut) {
    loop {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.put_u8(byte);
            return;
        }
        out.put_u8(byte | 0x80);
    }
}

/// The allocator counts every thread, so the tests take turns: one test's allocations are not another's.
static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Bytes allocated while `decode` runs over `body`.
fn allocated(body: Bytes, decode: impl FnOnce(&mut Bytes)) -> usize {
    let _turn = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut body = body;
    let region = Region::new(GLOBAL);
    decode(&mut body);
    region.change().bytes_allocated
}

#[test]
fn a_plain_array_count_the_buffer_cannot_hold_allocates_nothing_much() {
    let mut body = BytesMut::new();
    body.put_i32(i32::MAX); // Metadata v4: topics, claimed 2^31-1 times
    let used = allocated(body.freeze(), |b| {
        assert!(MetadataRequest::decode(b, 4).is_err());
    });
    assert!(used < BUDGET, "asked the allocator for {used} bytes");
}

#[test]
fn a_compact_array_count_the_buffer_cannot_hold_allocates_nothing_much() {
    let mut body = BytesMut::new();
    uvarint(u64::from(u32::MAX), &mut body); // Metadata v12: topics, claimed 2^32-2 times
    let used = allocated(body.freeze(), |b| {
        assert!(MetadataRequest::decode(b, 12).is_err());
    });
    assert!(used < BUDGET, "asked the allocator for {used} bytes");
}

#[test]
fn a_nested_array_count_the_buffer_cannot_hold_allocates_nothing_much() {
    let mut body = BytesMut::new();
    // JoinGroup v9: group id "g", timeouts, member id "", no instance id, protocol type "c",
    // then the protocols array claiming 2^32-2 elements.
    uvarint(2, &mut body);
    body.put_slice(b"g");
    body.put_i32(1000);
    body.put_i32(1000);
    uvarint(1, &mut body);
    uvarint(0, &mut body);
    uvarint(2, &mut body);
    body.put_u8(b'c');
    uvarint(u64::from(u32::MAX), &mut body);
    let used = allocated(body.freeze(), |b| {
        assert!(JoinGroupRequest::decode(b, 9).is_err());
    });
    assert!(used < BUDGET, "asked the allocator for {used} bytes");
}

/// Found by `fuzz_request` in under a minute: an IncrementalAlterConfigs v0 request whose first array
/// claims about 1.5 billion elements of 88 bytes each. The fuzzer was running the unpatched crate, which
/// asked for 137 GB; the input is kept to prove the whole read path no longer does.
#[test]
fn the_input_the_fuzzer_found_allocates_nothing_much() {
    use kafka_wire::incoming;
    use kafka_wire::profile::{Features, KafkaMajor, Profile};

    let frame = Bytes::from_static(include_bytes!(
        "data/fuzz-oom-incremental-alter-configs.bin"
    ));
    let used = allocated(Bytes::new(), |_| {
        let r = incoming::read(
            frame.clone(),
            Profile::new(KafkaMajor::DEFAULT),
            Features::default(),
        );
        assert!(r.is_err());
    });
    assert!(used < BUDGET, "asked the allocator for {used} bytes");
}

/// A count the buffer *can* hold is still a lie when each element is far bigger than the bytes that
/// describe it. A first version of the patch reserved one element per byte left, which an audit measured
/// at 76 times the body for Metadata; the reservation is now capped, so the decoder only ever holds what
/// it has really decoded.
#[test]
fn a_count_equal_to_the_bytes_left_allocates_about_the_body() {
    const BODY: usize = 1 << 20;
    let mut body = BytesMut::with_capacity(BODY);
    uvarint((BODY - 4) as u64, &mut body); // Metadata v12: topics, claimed once per byte left
    body.resize(BODY, 0);
    let used = allocated(body.freeze(), |b| {
        let _ = MetadataRequest::decode(b, 12);
    });
    assert!(
        used < 6 * BODY,
        "a {BODY}-byte body made the decoder allocate {used} bytes"
    );
}
