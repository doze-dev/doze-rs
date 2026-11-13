//! Tests of the number type.

use super::*;

fn n(s: &str) -> Number {
    Number::parse(s).unwrap()
}

#[test]
fn text_is_normalised() {
    assert_eq!(n("1.50").to_string(), "1.5");
    assert_eq!(n("0001").to_string(), "1");
    assert_eq!(n("-0").to_string(), "0");
    assert_eq!(n("1e3").to_string(), "1000");
    assert_eq!(n("+.5").to_string(), "0.5");
    assert_eq!(n("1E-7").to_string(), "1E-7");
    assert_eq!(
        n("12345678901234567890").to_string(),
        "12345678901234567890"
    );
}

#[test]
fn bad_text_and_limits() {
    for bad in ["", "abc", "1.2.3", "--1", "1e", ".", "1e+x"] {
        assert_eq!(Number::parse(bad), Err(NumberError::NotNumeric), "{bad}");
    }
    assert_eq!(Number::parse("1E126"), Err(NumberError::Overflow));
    assert_eq!(Number::parse("1E-131"), Err(NumberError::Underflow));
    assert!(Number::parse("1E125").is_ok() && Number::parse("1E-130").is_ok());
    assert_eq!(Number::parse(&"1".repeat(39)), Err(NumberError::TooPrecise));
    assert!(Number::parse(&"9".repeat(38)).is_ok());
}

#[test]
fn arithmetic_is_exact() {
    assert_eq!(n("0.1").add(&n("0.2")).unwrap(), n("0.3"));
    assert_eq!(n("1").sub(&n("3")).unwrap(), n("-2"));
    assert_eq!(n("-5").add(&n("5")).unwrap(), Number::zero());
    assert_eq!(n("99").add(&n("1")).unwrap(), n("100"));
    assert_eq!(n("1E125").add(&n("9E125")), Err(NumberError::Overflow));
    assert_eq!(n("2.5").sub(&n("-1.5")).unwrap(), n("4"));
}

#[test]
fn order_is_numeric_in_values_and_in_key_bytes() {
    let list = [
        "-100", "-2.5", "-2", "-0.001", "0", "0.001", "1", "1.5", "2", "10", "100", "1E20",
    ];
    let keys: Vec<Vec<u8>> = list
        .iter()
        .map(|s| {
            let mut k = Vec::new();
            n(s).order_key(&mut k);
            k
        })
        .collect();
    for w in keys.windows(2) {
        assert!(w[0] < w[1]);
    }
    for w in list.windows(2) {
        assert!(n(w[0]) < n(w[1]), "{} < {}", w[0], w[1]);
    }
}

#[test]
fn whole_part_and_size() {
    assert_eq!(n("1700000000").floor_i64(), Some(1_700_000_000));
    assert_eq!(n("1.9").floor_i64(), Some(1));
    assert_eq!(n("-1.5").floor_i64(), Some(-2));
    assert_eq!(n("12").size(), 2);
    assert_eq!(n("-123").size(), 4);
}
