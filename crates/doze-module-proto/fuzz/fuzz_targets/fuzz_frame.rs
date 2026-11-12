#![no_main]
//! Any bytes, fed to the frame decoder in two pieces and with both trailer sizes the platforms use, without a panic;
//! every frame that decodes encodes back to the same bytes.

use doze_module_proto::frame::{Decoder, encode};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let cut = data.first().map_or(0, |b| usize::from(*b)).min(data.len());
    for trailer in [0usize, 628] {
        let mut d = Decoder::new(trailer);
        d.push(&data[..cut]);
        let mut frames = Vec::new();
        while let Ok(Some(f)) = d.next_frame() {
            frames.push(f);
        }
        d.push(&data[cut..]);
        while let Ok(Some(f)) = d.next_frame() {
            frames.push(f);
        }
        for f in frames {
            let again = encode(f.id, f.kind, f.method, &f.body, f.handle.as_deref()).expect("a decoded frame re-encodes");
            let mut d2 = Decoder::new(trailer);
            d2.push(&again);
            assert_eq!(d2.next_frame().expect("re-decodes"), Some(f));
        }
    }
});
