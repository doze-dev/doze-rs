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
    let times: Vec<_> = b.record_times().unwrap().map(|r| r.unwrap()).collect();
    let got: Vec<_> = times
        .iter()
        .map(|t| (t.offset_delta, t.timestamp.get()))
        .collect();
    assert_eq!(got, [(0, 1_000), (1, 1_005), (2, 997), (3, 71_000)]);
}
