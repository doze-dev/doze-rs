use super::*;

#[test]
fn a_time_is_hours_and_minutes_in_utc() {
    assert_eq!(hhmm(1_000_000_000_000), "01:46");
    assert_eq!(hhmm(0), "00:00");
}
