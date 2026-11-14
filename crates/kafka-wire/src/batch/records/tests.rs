use crate::batch::Batch;
use crate::batch::build::BatchBuilder;

fn zigzag(n: i64) -> Vec<u8> {
    let mut v = ((n << 1) ^ (n >> 63)) as u64;
    let mut out = Vec::new();
    loop {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(byte);
            return out;
        }
        out.push(byte | 0x80);
    }
}

/// A record with a key and value, as a producer writes it.
fn record(timestamp_delta: i64, offset_delta: i64) -> Vec<u8> {
    let mut body = vec![0u8]; // attributes
    body.extend(zigzag(timestamp_delta));
    body.extend(zigzag(offset_delta));
    body.extend(zigzag(3)); // a key of 3 bytes
    body.extend(b"key");
    body.extend(zigzag(-1)); // a null value
    body.extend(zigzag(0)); // no headers
    let mut out = zigzag(body.len() as i64);
    out.extend(body);
    out
}

fn batch(first: i64, deltas: &[(i64, i64)]) -> Batch {
    let body: Vec<u8> = deltas.iter().flat_map(|(t, o)| record(*t, *o)).collect();
    let bytes = BatchBuilder::new()
        .records(deltas.len() as u32)
        .timestamps(first, first)
        .body(body)
        .build();
    Batch::parse(bytes).unwrap()
}

#[test]
fn each_record_has_its_offset_delta_and_the_batchs_first_timestamp_plus_its_own_delta() {
    let b = batch(1_000, &[(0, 0), (5, 1), (-3, 2), (70_000, 3)]);
    let times: Vec<_> = b.record_times(1 << 20).map(|r| r.unwrap()).collect();
    let got: Vec<_> = times
        .iter()
        .map(|t| (t.offset_delta, t.timestamp.get()))
        .collect();
    assert_eq!(got, [(0, 1_000), (1, 1_005), (2, 997), (3, 71_000)]);
}

#[test]
fn a_timestamp_delta_whose_tenth_byte_carries_more_than_the_last_bit_is_unreadable() {
    let mut body = vec![0u8]; // attributes
    body.extend([0xff; 9]);
    body.push(0x02); // no 64-bit number
    body.extend(zigzag(0)); // offset delta
    body.extend(zigzag(-1));
    body.extend(zigzag(-1));
    body.extend(zigzag(0));
    let mut section = zigzag(body.len() as i64);
    section.extend(body);
    let bytes = BatchBuilder::new()
        .records(1)
        .timestamps(0, 0)
        .body(section)
        .build();
    let b = Batch::parse(bytes).unwrap();
    let times: Vec<_> = b.record_times(1 << 20).collect();
    assert_eq!(times, [Err(())]);
}

/// The batch `batch` builds, its attribute bits set and its header's newest timestamp `max`.
fn with(first: i64, max: i64, attributes: u16, body: Vec<u8>, count: u32) -> Batch {
    let bytes = BatchBuilder::new()
        .records(count)
        .attributes(attributes)
        .timestamps(first, max)
        .body(body)
        .build();
    Batch::parse(bytes).unwrap()
}

fn times_of(b: &Batch) -> Vec<(i32, i64)> {
    b.record_times(1 << 20)
        .map(|r| r.unwrap())
        .map(|t| (t.offset_delta, t.timestamp.get()))
        .collect()
}

#[test]
fn under_log_append_time_every_record_has_the_batchs_newest_timestamp() {
    // DefaultRecordBatch: `logAppendTime = timestampType() == LOG_APPEND_TIME ? maxTimestamp() : null`, and a record
    // with a log append time reports it in place of base timestamp + delta.
    let body: Vec<u8> = [(0, 0), (5, 1)].iter().flat_map(|(t, o)| record(*t, *o)).collect();
    let b = with(1_000, 9_000, 0x08, body, 2);
    assert_eq!(times_of(&b), [(0, 9_000), (1, 9_000)]);
}

#[test]
fn a_compressed_batch_is_walked_through_its_codec_to_the_record() {
    use std::io::Write;
    let plain: Vec<u8> = [(0, 0), (100, 1), (300, 2)]
        .iter()
        .flat_map(|(t, o)| record(*t, *o))
        .collect();
    let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(&plain).unwrap();
    let b = with(1_000, 1_300, 0x01, e.finish().unwrap(), 3);
    assert_eq!(times_of(&b), [(0, 1_000), (1, 1_100), (2, 1_300)]);
    // Held to the limit: past it the walk ends with an error, not a guess.
    let capped: Vec<_> = b.record_times(4).collect();
    assert_eq!(capped, [Err(())]);
}
