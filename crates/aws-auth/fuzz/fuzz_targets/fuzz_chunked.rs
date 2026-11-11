#![no_main]
//! Any bytes as an aws-chunked body, fed in pieces of any size: no panic, and the decoder never claims more than it was given.

use aws_auth::chunked::{ChunkDecoder, Event};
use bytes::Bytes;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((&piece, body)) = data.split_first() else { return };
    let piece = usize::from(piece % 64) + 1;
    let mut decoder = ChunkDecoder::new(None, false);
    let mut payload = 0usize;
    'feed: for part in body.chunks(piece) {
        let mut input = Bytes::copy_from_slice(part);
        loop {
            match decoder.next(&mut input) {
                Ok(Event::Data(bytes)) => payload += bytes.len(),
                Ok(Event::Need) => break,
                Ok(Event::Done) | Err(_) => break 'feed,
            }
        }
    }
    assert!(payload <= body.len());
});
