//! The vectors are Kafka 4.4.0's own arithmetic: `Sensor`, `Rate`, `TokenBucket` and `QuotaUtils.throttleTime` from its jars,
//! driven with these times and values (`tools/kafka-oracle/QuotaMath.java`; its output is quoted in each
//! test).

use super::*;

const T: i64 = 1_700_000_000_000;

/// Records each value at its time against `bound` as `ClientQuotaManager.recordAndGetThrottleTimeMs` does (record, then
/// measure), and returns the measured rate and the throttle of each step.
fn run_rate(bound: f64, steps: &[(i64, f64)]) -> Vec<(f64, i64)> {
    let mut rate = Rate::default();
    steps
        .iter()
        .map(|&(now, value)| {
            rate.record(value, now);
            let measured = rate.measure(now);
            let throttle = if measured <= bound {
                0
            } else {
                throttle_ms(measured, bound, rate.window_size(now))
            };
            (measured, throttle)
        })
        .collect()
}

fn assert_steps(got: &[(f64, i64)], want: &[(f64, i64)]) {
    assert_eq!(got.len(), want.len());
    for (i, (g, w)) in got.iter().zip(want).enumerate() {
        assert!(
            (g.0 - w.0).abs() < 1e-6,
            "step {i}: measured {} want {}",
            g.0,
            w.0
        );
        assert_eq!(g.1, w.1, "step {i}: throttle");
    }
}

#[test]
fn a_burst_is_measured_over_the_whole_span_and_throttled_for_the_time_it_takes_to_come_back() {
    // rate burst bound=1024.0: 500.000000/0, 999.000999/0, 961.904762/0, 1110.000000/840, 10.000000/0
    let got = run_rate(
        1024.0,
        &[
            (T, 5000.0),
            (T + 10, 5000.0),
            (T + 500, 100.0),
            (T + 3000, 1000.0),
            (T + 20000, 100.0),
        ],
    );
    assert_steps(
        &got,
        &[
            (500.0, 0),
            (999.000_999, 0),
            (961.904_762, 0),
            (1110.0, 840),
            (10.0, 0),
        ],
    );
}

#[test]
fn a_steady_client_under_its_quota_is_never_throttled_and_a_spike_is() {
    // rate steady bound=1000.0: 80, 160, ... 880, 880, then 1200/2000, 1152.380952/1600, 0.1/0
    let mut steps: Vec<(i64, f64)> = (0..12).map(|i| (T + i * 1000, 800.0)).collect();
    steps.extend([(T + 12000, 4000.0), (T + 12500, 100.0), (T + 30000, 1.0)]);
    let mut want: Vec<(f64, i64)> = (1..=11).map(|i| (f64::from(i) * 80.0, 0)).collect();
    want.extend([(880.0, 0), (1200.0, 2000), (1_152.380_952, 1600), (0.1, 0)]);
    assert_steps(&run_rate(1000.0, &steps), &want);
}

#[test]
fn windows_roll_and_an_unrecorded_value_takes_its_bytes_back() {
    // rate windows bound=100.0: 5/0, 9.091736/0, 210/11000, 9.999/0, 10/0, 1.818347/0, 274.545455/19200
    let got = run_rate(
        100.0,
        &[
            (T, 50.0),
            (T + 999, 50.0),
            (T + 1000, 2000.0),
            (T + 1001, -2000.0),
            (T + 11000, 10.0),
            (T + 11999, 10.0),
            (T + 22000, 3000.0),
        ],
    );
    assert_steps(
        &got,
        &[
            (5.0, 0),
            (9.091_736, 0),
            (210.0, 11000),
            (9.999, 0),
            (10.0, 0),
            (1.818_347, 0),
            (274.545_455, 19200),
        ],
    );
}

#[test]
fn the_token_bucket_takes_a_mutation_that_empties_it_and_refuses_the_next_until_it_is_back_at_zero()
{
    // bucket mutations bound=10.0, strict (check, then record):
    //   taken 60, taken -9.99, refused 998 -9.98, refused 0 -0.000000 (a rounding below zero), taken 19, taken 24,
    //   taken -175, taken 83
    let mut bucket = TokenBucket::default();
    let mut out = Vec::new();
    for (now, permits) in [
        (T, 50.0),
        (T + 1, 70.0),
        (T + 2, 1.0),
        (T + 1000, 1.0),
        (T + 3000, 1.0),
        (T + 4000, 5.0),
        (T + 4100, 200.0),
        (T + 30000, 1.0),
    ] {
        let tokens = bucket.measure(10.0, now);
        if tokens < 0.0 {
            out.push(Some(token_throttle_ms(tokens, 10.0)));
        } else {
            bucket.record(permits, 10.0, now);
            out.push(None);
        }
        let left = bucket.measure(10.0, now);
        out.push(Some(java_round(left * 1000.0)));
    }
    let want: Vec<Option<i64>> = vec![
        None,
        Some(60_000),
        None,
        Some(-9990),
        Some(998),
        Some(-9980),
        Some(0),
        Some(0),
        None,
        Some(19_000),
        None,
        Some(24_000),
        None,
        Some(-175_000),
        None,
        Some(83_000),
    ];
    assert_eq!(out, want);
}

#[test]
fn rounding_is_javas() {
    assert_eq!(java_round(0.5), 1);
    assert_eq!(java_round(-0.5), 0);
    assert_eq!(java_round(-2.5), -2);
    assert_eq!(java_round(839.84), 840);
    assert_eq!(java_round(f64::NAN), 0);
    assert_eq!(java_round(1e300), i64::MAX);
}

#[test]
fn an_empty_rate_measures_nothing_over_the_least_span() {
    let mut rate = Rate::default();
    assert_eq!(rate.measure(T), 0.0);
    assert_eq!(rate.window_size(T), 10_000);
}
