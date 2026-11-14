use super::*;

fn n(s: &str) -> Number {
    Number::parse(s).unwrap()
}

#[test]
fn prints_the_shortest_decimal_that_reads_back() {
    assert_eq!(n("0.1").add(&n("0.2")).unwrap().text(), "0.3");
    assert_eq!(n("1").div(&n("3")).unwrap().text().len(), 157); // "0." and 155 digits
    assert_eq!(
        n("123456789012345678901234567890").text(),
        "123456789012345678901234567890"
    );
    assert_eq!(n("1e3").text(), "1000");
    assert_eq!(n("1.5e-3").text(), "0.0015");
    assert_eq!(n("-0").text(), "-0");
}

#[test]
fn parses_as_go_does() {
    assert!(Number::parse("").is_none());
    assert!(Number::parse(".").is_none());
    assert!(Number::parse("1e").is_none());
    assert!(Number::parse(" 1").is_none());
    assert!(Number::parse("0x10").is_none());
    assert_eq!(n(".5").text(), "0.5");
    assert_eq!(n("1.").text(), "1");
    assert_eq!(n("1p3").text(), "8");
    assert_eq!(n("+Inf").text(), "+Inf");
    assert_eq!(n("-inf").text(), "-Inf");
    assert!(Number::parse("1e9999999999").is_none());
}

#[test]
fn divides_and_takes_remainders_like_cty() {
    assert_eq!(n("5").div(&n("0")).unwrap().text(), "+Inf");
    assert_eq!(n("0").div(&n("0")), Err(ArithError::ZeroOverZero));
    assert_eq!(n("5").modulo(&n("0")).unwrap().text(), "5");
    assert_eq!(n("-7").modulo(&n("3")).unwrap().text(), "-1");
    assert_eq!(n("7.5").modulo(&n("2")).unwrap().text(), "1.5");
    let inf = n("1").div(&n("0")).unwrap();
    assert_eq!(inf.modulo(&n("2")), Err(ArithError::ModuloInfinity));
}

#[test]
fn compares_by_value_and_equals_by_print() {
    assert_eq!(n("-0").cmp_value(&n("0")), Ordering::Equal);
    assert!(n("-0").same(&n("0")));
    assert!(Number::from_i64(3).same(&n("3")));
    // different bits, same print: equal, as in cty
    assert!(n("0.1").same(&Number::from_f64(0.1).unwrap()));
    assert_eq!(n("2").cmp_value(&n("10")), Ordering::Less);
}

#[test]
fn host_numbers_keep_their_precision() {
    assert_eq!(Number::from_i64(-42).text(), "-42");
    assert_eq!(Number::from_f64(0.1).unwrap().text(), "0.1");
    assert_eq!(Number::from_f64(1e300).unwrap().text().len(), 301);
    assert_eq!(Number::from_i64(i64::MIN).to_i64(), Some(i64::MIN));
}

#[test]
fn go_g10_format() {
    assert_eq!(n("1").text_g10(), "1");
    assert_eq!(n("123456789012").text_g10(), "1.23456789e+11");
    assert_eq!(n("0.00001").text_g10(), "1e-05");
    assert_eq!(n("3.14159265358979").text_g10(), "3.141592654");
}

#[test]
fn huge_exponents_stay_bounded() {
    let big = n("1e39000");
    assert!(decimal::exact_digits(&big.0) < 40_000);
    let tiny = n("1e-39000");
    let started = std::time::Instant::now();
    assert!(tiny.text().len() > 39_000);
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
    assert_eq!(n("1e40000").text(), "+Inf");
    assert_eq!(n("1e-40000").text(), "0");
    assert_eq!(n("-1e-40000").text(), "-0");
}
