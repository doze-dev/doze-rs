#![no_main]
//! Any file as a bbolt database: an answer or an error, never a panic, a hang or an allocation past the input's size
//! (every walk is bounded by the pages the file says it has).

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(db) = doze_bolt::Db::parse(data.to_vec()) {
        let _ = db.holds_data();
        if let Ok(top) = db.buckets(&[]) {
            for name in top.iter().take(8) {
                let _ = db.values(&[name]);
                if let Ok(inner) = db.buckets(&[name]) {
                    for sub in inner.iter().take(4) {
                        let _ = db.items(&[name, sub]);
                    }
                }
            }
        }
    }
});
