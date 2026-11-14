//! The gate and the store, with the clock in the test's hands.

use super::*;

const A: [u8; 16] = [1; 16];
const B: [u8; 16] = [2; 16];

fn after(t: Instant, seconds: f64) -> Instant {
    t + Duration::from_secs_f64(seconds)
}

#[test]
fn an_idle_store_holds_nothing() {
    let store = Telemetry::default();
    assert_eq!(store.held(), (0, 0));
    assert!(store.lock().instances.is_empty());
}

#[test]
fn the_first_push_is_on_time_the_next_is_early_until_an_interval_has_passed() {
    let (store, t) = (Telemetry::default(), Instant::now());
    assert_eq!(store.arrive(A, false, t), Verdict::Accepted);
    assert_eq!(store.arrive(A, false, after(t, 0.0)), Verdict::Early);
    assert_eq!(store.arrive(A, false, after(t, 2.5)), Verdict::Early);
    assert_eq!(store.arrive(A, false, after(t, 4.9)), Verdict::Early);
    // Recorded (telemetry-interval): the early ones did not postpone the one that is on time.
    assert_eq!(store.arrive(A, false, after(t, 5.0)), Verdict::Accepted);
    assert_eq!(store.arrive(A, false, after(t, 5.1)), Verdict::Early);
}

#[test]
fn an_instance_is_the_only_thing_the_gate_counts_so_others_are_not_held_back() {
    let (store, t) = (Telemetry::default(), Instant::now());
    assert_eq!(store.arrive(A, false, t), Verdict::Accepted);
    assert_eq!(store.arrive(B, false, t), Verdict::Accepted);
    assert_eq!(store.arrive(A, false, t), Verdict::Early);
}

#[test]
fn a_subscription_given_since_the_last_push_makes_the_next_push_on_time() {
    let (store, t) = (Telemetry::default(), Instant::now());
    assert_eq!(store.arrive(A, false, t), Verdict::Accepted);
    // A subscription is early too (it follows a push), so it gives nothing.
    assert_eq!(store.subscribe(A, after(t, 1.0)), Verdict::Early);
    assert_eq!(store.arrive(A, false, after(t, 1.5)), Verdict::Early);
    // After an interval it is given, and a push soon after it is on time though the last push is only just old enough.
    assert_eq!(store.subscribe(A, after(t, 5.5)), Verdict::Accepted);
    assert_eq!(store.arrive(A, false, after(t, 6.0)), Verdict::Accepted);
    assert_eq!(store.arrive(A, false, after(t, 6.5)), Verdict::Early);
}

#[test]
fn a_subscription_request_shares_the_clock_with_pushes_and_a_refused_one_postpones_nothing() {
    let (store, t) = (Telemetry::default(), Instant::now());
    assert_eq!(store.subscribe(A, t), Verdict::Accepted);
    assert_eq!(store.subscribe(A, after(t, 2.0)), Verdict::Early);
    assert_eq!(store.subscribe(A, after(t, 4.9)), Verdict::Early);
    assert_eq!(store.subscribe(A, after(t, 5.0)), Verdict::Accepted);
    // A push right after a subscription is the first push, so on time; a subscription right after that push is early.
    assert_eq!(store.arrive(A, false, after(t, 5.1)), Verdict::Accepted);
    assert_eq!(store.subscribe(A, after(t, 5.2)), Verdict::Early);
}

#[test]
fn a_terminating_push_is_taken_early_and_closes_the_instance_for_the_next_push_only() {
    let (store, t) = (Telemetry::default(), Instant::now());
    assert_eq!(store.arrive(A, false, t), Verdict::Accepted);
    assert_eq!(store.arrive(A, true, after(t, 1.0)), Verdict::Accepted);
    assert_eq!(
        store.arrive(A, false, after(t, 1.1)),
        Verdict::AfterTerminating
    );
    // The refused push said it was not terminating: the instance is open again, and the interval has not passed.
    assert_eq!(store.arrive(A, false, after(t, 1.2)), Verdict::Early);
}

#[test]
fn terminating_pushes_in_a_row_are_refused_from_the_second_on() {
    let (store, t) = (Telemetry::default(), Instant::now());
    assert_eq!(store.arrive(A, true, t), Verdict::Accepted);
    for n in 1..4 {
        assert_eq!(
            store.arrive(A, true, after(t, f64::from(n))),
            Verdict::AfterTerminating,
            "terminating push {n}"
        );
    }
    assert_eq!(
        store.arrive(A, false, after(t, 4.0)),
        Verdict::AfterTerminating
    );
}

#[test]
fn a_subscription_request_does_not_clear_a_terminating_push() {
    let (store, t) = (Telemetry::default(), Instant::now());
    assert_eq!(store.arrive(A, true, t), Verdict::Accepted);
    let _ = store.subscribe(A, after(t, 6.0));
    assert_eq!(
        store.arrive(A, false, after(t, 6.1)),
        Verdict::AfterTerminating
    );
}

#[test]
fn the_request_after_a_refusal_that_says_to_ask_again_is_answered_once_and_sets_no_clock() {
    let (store, t) = (Telemetry::default(), Instant::now());
    assert_eq!(store.subscribe(A, t), Verdict::Accepted);
    // A push that got through the gate (and, the handler found, was refused for its subscription id).
    assert_eq!(store.arrive(A, false, after(t, 0.5)), Verdict::Accepted);
    store.ask_again(&A);
    assert_eq!(store.subscribe(A, after(t, 0.6)), Verdict::Accepted);
    assert_eq!(store.subscribe(A, after(t, 0.7)), Verdict::Early);
    // It set no clock: the push straight after is judged by the push before it.
    assert_eq!(store.arrive(A, false, after(t, 0.8)), Verdict::Early);
}

#[test]
fn a_push_that_comes_next_cancels_the_ask_again() {
    let (store, t) = (Telemetry::default(), Instant::now());
    assert_eq!(store.arrive(A, false, t), Verdict::Accepted);
    store.ask_again(&A);
    assert_eq!(store.arrive(A, false, after(t, 1.0)), Verdict::Early);
    assert_eq!(store.subscribe(A, after(t, 1.1)), Verdict::Early);
}

#[test]
fn asking_again_about_an_unknown_instance_makes_no_instance() {
    let store = Telemetry::default();
    store.ask_again(&A);
    assert_eq!(store.held(), (0, 0));
}

#[test]
fn the_subscription_id_is_a_function_of_the_instance_alone() {
    assert_eq!(subscription_id(&A), subscription_id(&A));
    let ids: std::collections::BTreeSet<i32> =
        (0..=255_u8).map(|n| subscription_id(&[n; 16])).collect();
    assert_eq!(
        ids.len(),
        256,
        "a checksum of 16 bytes does not repeat over 256 of them"
    );
}

#[test]
fn the_last_payload_replaces_the_one_before_and_is_counted() {
    let (store, t) = (Telemetry::default(), Instant::now());
    store.keep(A, Bytes::from_static(&[0; 100]), t);
    assert_eq!(store.held(), (1, 100));
    store.keep(A, Bytes::from_static(&[0; 40]), t);
    assert_eq!(store.held(), (1, 40));
    store.keep(B, Bytes::from_static(&[0; 10]), t);
    assert_eq!(store.held(), (2, 50));
}

#[test]
fn bytes_are_bounded_and_the_payloads_heard_longest_ago_go_first() {
    let (store, t) = (Telemetry::default(), Instant::now());
    let big = Bytes::from(vec![0_u8; MAX_PAYLOAD]);
    for n in 0..4_u8 {
        store.keep([n; 16], big.clone(), after(t, f64::from(n)));
    }
    assert_eq!(store.held(), (4, MAX_BYTES));
    // A fifth does not fit: the oldest payload is let go, its instance (and so its gate) kept.
    store.keep([9; 16], big, after(t, 4.0));
    assert_eq!(store.held(), (5, MAX_BYTES));
    let state = store.lock();
    assert!(state.instances[&[0; 16]].payload.is_empty());
    assert_eq!(state.instances[&[9; 16]].payload.len(), MAX_PAYLOAD);
}

#[test]
fn entries_are_bounded_and_a_terminating_client_goes_before_the_one_heard_longest_ago() {
    let (store, t) = (Telemetry::default(), Instant::now());
    for n in 0..MAX_INSTANCES {
        let id = [u8::try_from(n).unwrap(); 16];
        let _ = store.arrive(id, n == 77, after(t, n as f64 * 0.001));
    }
    assert_eq!(store.held().0, MAX_INSTANCES);
    let _ = store.arrive([200; 16], false, after(t, 1.0));
    assert_eq!(store.held().0, MAX_INSTANCES);
    let state = store.lock();
    assert!(
        !state.instances.contains_key(&[77; 16]),
        "the terminating one went"
    );
    assert!(state.instances.contains_key(&[0; 16]), "the oldest stayed");
    drop(state);
    // With none terminating, the one heard from longest ago goes.
    let _ = store.arrive([201; 16], false, after(t, 1.1));
    assert!(!store.lock().instances.contains_key(&[0; 16]));
    assert_eq!(store.held().0, MAX_INSTANCES);
}

#[test]
fn evicting_an_instance_frees_its_bytes() {
    let (store, t) = (Telemetry::default(), Instant::now());
    for n in 0..MAX_INSTANCES {
        let id = [u8::try_from(n).unwrap(); 16];
        store.keep(
            id,
            Bytes::from_static(&[0; 1000]),
            after(t, n as f64 * 0.001),
        );
    }
    assert_eq!(store.held(), (MAX_INSTANCES, MAX_INSTANCES * 1000));
    let _ = store.arrive([250; 16], false, after(t, 1.0));
    assert_eq!(store.held(), (MAX_INSTANCES, (MAX_INSTANCES - 1) * 1000));
}

#[test]
fn a_client_that_went_quiet_is_forgotten_the_next_time_anyone_calls() {
    let (store, t) = (Telemetry::default(), Instant::now());
    store.keep(A, Bytes::from_static(&[0; 100]), t);
    let _ = store.arrive(B, true, after(t, 1.0));
    // Terminating clients are forgotten after a minute, the others after ten.
    let _ = store.subscribe([3; 16], after(t, 70.0));
    assert!(!store.lock().instances.contains_key(&B));
    assert_eq!(store.held(), (2, 100));
    let _ = store.subscribe([4; 16], after(t, 700.0));
    assert!(!store.lock().instances.contains_key(&A));
    assert_eq!(
        store.held(),
        (1, 0),
        "[3] was heard at 70 s: gone too, by 700"
    );
}

#[test]
fn nothing_prints_a_payload_or_an_instance_id() {
    let (store, t) = (Telemetry::default(), Instant::now());
    store.keep(A, Bytes::from_static(b"planted-telemetry-secret-4417"), t);
    let of_the_store = format!("{store:?}");
    let of_the_instance = format!("{:?}", store.lock().instances[&A]);
    let said = format!("{of_the_store} {of_the_instance}");
    for planted in ["planted", "4417", "[1, 1", "01010101"] {
        assert!(!said.contains(planted), "{planted:?} in {said}");
    }
    assert!(said.contains("payload_bytes"), "{said}");
}
