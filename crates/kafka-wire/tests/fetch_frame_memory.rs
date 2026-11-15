//! A Fetch frame does not copy its records: writing the frame for 50 MiB of records allocates the envelope and nothing
//! the size of the records, where the one-buffer encoding allocates all of it again.
#![allow(clippy::unwrap_used)]

use std::alloc::System;

use bytes::Bytes;
use kafka_protocol::messages::FetchResponse;
use kafka_protocol::messages::fetch_response::{FetchableTopicResponse, PartitionData};
use kafka_wire::api::Response;
use kafka_wire::response;
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

const RECORDS: usize = 50 << 20;

/// One test in this file: the allocator counts every thread, so nothing else may run while it measures.
#[test]
fn a_fetch_frame_allocates_its_envelope_and_not_its_records() {
    let mut partitions = Vec::new();
    for i in 0..4 {
        let mut p = PartitionData::default();
        p.partition_index = i;
        p.records = Some(Bytes::from(vec![1_u8; RECORDS / 4]));
        partitions.push(p);
    }
    let mut t = FetchableTopicResponse::default();
    t.partitions = partitions;
    let mut r = FetchResponse::default();
    r.responses = vec![t];
    let response = Response::Fetch(r);
    for version in [4, 11, 12, 13] {
        let region = Region::new(GLOBAL);
        let frame = response::frame(1, version, &response).unwrap();
        let framed = region.change().bytes_allocated;
        drop(frame);
        let region = Region::new(GLOBAL);
        let whole = response::encode(1, version, &response).unwrap();
        let copied = region.change().bytes_allocated;
        drop(whole);
        assert!(
            framed < 4 << 10,
            "v{version}: the frame allocated {framed} B"
        );
        assert!(
            copied > RECORDS,
            "v{version}: the one buffer allocated {copied} B"
        );
    }
}
