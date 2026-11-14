#![no_main]
//! Any bytes, pushed to the NDJSON decoder in two pieces, read as frames and as requests without a panic; every frame
//! that decodes re-encodes to a line that decodes to the same frame.

use doze_control::codec::{Decoder, encode};
use doze_control::{Frame, Request};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let cut = data.first().map_or(0, |b| usize::from(*b)).min(data.len());
    let mut frames = Decoder::with_limit(1 << 16);
    let mut requests = Decoder::with_limit(1 << 16);
    for part in [&data[..cut], &data[cut..]] {
        frames.push(part);
        requests.push(part);
        loop {
            match frames.next_frame::<Frame>() {
                Ok(Some(f)) => {
                    let line = encode(&f).expect("a decoded frame encodes");
                    let mut again = Decoder::new();
                    again.push(&line);
                    assert_eq!(again.next_frame::<Frame>().expect("re-decodes"), Some(f));
                }
                Ok(None) => break,
                Err(_) => {}
            }
        }
        while !matches!(requests.next_frame::<Request>(), Ok(None)) {}
    }
});
