#![no_main]
//! Any compose file or Makefile reads without a panic: every service found has an image, every target a name.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    for (svc, image) in doze_desk::act::compose_images(&text) {
        assert!(!image.is_empty(), "{svc}");
    }
    for t in doze_desk::act::make_targets(&text) {
        assert!(!t.is_empty());
    }
});
