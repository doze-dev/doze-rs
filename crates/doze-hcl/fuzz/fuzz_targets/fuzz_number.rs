//! Any two strings: parsed as numbers (Go's rules), combined by every operator and printed, both
//! ways; never panics or hangs. (Printing does not always read back: Go's shortest-digit search
//! bounds a power of two symmetrically, and doze prints what Go prints; see number/tests.rs.)
#![no_main]

use doze_hcl::Number;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: (&str, &str)| {
    let (Some(a), Some(b)) = (Number::parse(data.0), Number::parse(data.1)) else {
        return;
    };
    let results = [a.add(&b), a.sub(&b), a.mul(&b), a.div(&b), a.modulo(&b)];
    for n in results.into_iter().flatten().chain([a.neg(), b.abs()]) {
        let _ = n.text();
        let _ = n.text_g10();
        let _ = (n.to_i64(), n.is_integer(), n.cmp_value(&a), n.same(&b));
    }
});
