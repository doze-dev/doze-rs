//! Any two strings: parsed as numbers (Go's rules) and combined by every operator, then printed;
//! never panics, and printing reads back to the same number.
#![no_main]

use doze_hcl::Number;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: (&str, &str)| {
    let (Some(a), Some(b)) = (Number::parse(data.0), Number::parse(data.1)) else { return };
    let results = [a.add(&b), a.sub(&b), a.mul(&b), a.div(&b), a.modulo(&b)];
    for n in results.into_iter().flatten().chain([a.neg(), b.abs()]) {
        let text = n.text();
        let _ = n.text_g10();
        if let Some(back) = Number::parse(&text) {
            assert!(back.same(&n) || n.precision() < doze_hcl::number::PARSE_PREC, "{text}");
        }
    }
});
