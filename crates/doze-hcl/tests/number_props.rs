//! Properties of numbers that hold for any input, beside the Go corpus: printing reads back to the
//! same number, addition and multiplication commute, and the order agrees with subtraction's sign.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use doze_hcl::Number;
use proptest::prelude::*;
use std::cmp::Ordering;

fn decimal() -> impl Strategy<Value = String> {
    (
        any::<bool>(),
        "[0-9]{1,40}",
        proptest::option::of("[0-9]{1,40}"),
        proptest::option::of(-400i32..400),
    )
        .prop_map(|(neg, int, frac, exp)| {
            let mut s = String::new();
            if neg {
                s.push('-');
            }
            s.push_str(&int);
            if let Some(f) = frac {
                s.push('.');
                s.push_str(&f);
            }
            if let Some(e) = exp {
                s.push_str(&format!("e{e}"));
            }
            s
        })
}

proptest! {
    #[test]
    fn printing_reads_back(s in decimal()) {
        let n = Number::parse(&s).unwrap();
        let back = Number::parse(&n.text()).unwrap();
        prop_assert!(back.same(&n), "{s} printed {} read back {}", n.text(), back.text());
        prop_assert_eq!(back.cmp_value(&n), Ordering::Equal);
    }

    #[test]
    fn add_and_mul_commute(a in decimal(), b in decimal()) {
        let (a, b) = (Number::parse(&a).unwrap(), Number::parse(&b).unwrap());
        prop_assert!(a.add(&b).unwrap().same(&b.add(&a).unwrap()));
        prop_assert!(a.mul(&b).unwrap().same(&b.mul(&a).unwrap()));
    }

    #[test]
    fn order_agrees_with_subtraction(a in decimal(), b in decimal()) {
        let (a, b) = (Number::parse(&a).unwrap(), Number::parse(&b).unwrap());
        let diff = a.sub(&b).unwrap();
        let by_sign = diff.signum().cmp(&0);
        prop_assert_eq!(a.cmp_value(&b), by_sign);
    }
}
