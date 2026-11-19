use std::time::Duration;

use aws_gateway::Finished;

use super::*;

fn call(n: u64) -> Finished<'static> {
    Finished {
        request_id: "0c6f3a52-7a8b-4c1d-9e2f-000000000000",
        signing_name: "sqs",
        sdk_id: "SQS",
        action: "SendMessage",
        resource: "",
        access_key_id: "test",
        region: "us-east-1",
        status: 200,
        code: "",
        took: Duration::from_micros(n),
    }
}

fn seqs(page: &Page) -> Vec<u64> {
    page.calls.iter().map(|c| c.seq).collect()
}

#[test]
fn calls_are_numbered_from_one_and_read_back_in_order() {
    let r = Recorder::holding(8);
    assert_eq!(r.newest_seq(), 0);
    assert_eq!(r.after(0, 10), Page::default());
    for n in 1..=3 {
        assert_eq!(r.record(&call(n), 1_000 + n), n);
    }
    let all = r.after(0, 10);
    assert_eq!(seqs(&all), [1, 2, 3]);
    assert_eq!((all.last, all.missed, all.more), (3, 0, false));
    let first = &all.calls[0];
    assert_eq!((first.at_ms, first.took_us, first.status), (1_001, 1, 200));
    assert_eq!((first.signing_name, first.action), ("sqs", "SendMessage"));
    assert_eq!((&*first.caller, &*first.region), ("test", "us-east-1"));
    // A reader that has the first is given the rest, and one that has them all is given nothing and keeps its place.
    assert_eq!(seqs(&r.after(1, 10)), [2, 3]);
    let none = r.after(3, 10);
    assert!(none.calls.is_empty());
    assert_eq!((none.last, none.missed, none.more), (3, 0, false));
}

#[test]
fn a_page_is_cut_at_its_limit_and_says_there_is_more() {
    let r = Recorder::holding(8);
    for n in 1..=5 {
        r.record(&call(n), n);
    }
    let page = r.after(1, 2);
    assert_eq!(seqs(&page), [2, 3]);
    assert_eq!((page.last, page.more), (3, true));
    let rest = r.after(page.last, 2);
    assert_eq!(seqs(&rest), [4, 5]);
    assert!(!rest.more);
    assert_eq!(seqs(&r.newest(2)), [4, 5]);
    assert_eq!(seqs(&r.newest(100)), [1, 2, 3, 4, 5]);
}

#[test]
fn the_ring_holds_its_capacity_and_no_more_and_says_what_a_reader_missed() {
    let r = Recorder::holding(4);
    for n in 1..=10 {
        r.record(&call(n), n);
    }
    let fill = r.fill();
    assert_eq!((fill.held, fill.recorded), (4, 10));
    // A reader that had the second call: 3 to 6 are gone, 7 to 10 are given.
    let page = r.after(2, 100);
    assert_eq!(seqs(&page), [7, 8, 9, 10]);
    assert_eq!((page.missed, page.last, page.more), (4, 10, false));
    // One that asks for nothing is still moved past what is gone.
    assert_eq!(r.after(2, 0).last, 6);
    // The newest are never "missed".
    assert_eq!(r.newest(100).missed, 0);
}

#[test]
fn the_bytes_held_are_counted_and_never_pass_the_bound() {
    let r = Recorder::holding(3);
    assert_eq!(r.fill().bytes, 0);
    let long = "k".repeat(5_000);
    let big = Finished {
        request_id: &long,
        resource: &long,
        access_key_id: &long,
        region: &long,
        code: &long,
        ..call(1)
    };
    for n in 1..=20 {
        r.record(&big, n);
        let fill = r.fill();
        let held: usize = r.newest(100).calls.iter().map(Call::bytes).sum();
        assert_eq!(fill.bytes, held);
        assert!(fill.bytes <= 3 * (RECORD_BYTES + MOST_TEXT), "{fill:?}");
    }
    let one = &r.newest(1).calls[0];
    assert_eq!(one.bytes(), RECORD_BYTES + MOST_TEXT);
    assert_eq!(one.resource.len(), MOST_RESOURCE);
    assert_eq!(one.caller.len(), MOST_CALLER);
    // The whole ring, at its worst, is under a megabyte.
    const { assert!(MOST_BYTES < 1 << 20) };
    const { assert!(RECORD_BYTES <= 160) };
}

#[test]
fn text_is_cut_on_a_character_boundary() {
    let r = Recorder::holding(2);
    // 'é' is two bytes: 300 of them cut at 256 bytes is 128 whole characters, and cut at 31 would split one.
    let key = "é".repeat(300);
    let odd = format!("a{}", "é".repeat(40));
    r.record(
        &Finished {
            resource: &key,
            region: &odd,
            ..call(1)
        },
        1,
    );
    let got = &r.newest(1).calls[0];
    assert_eq!(got.resource.chars().count(), 128);
    assert_eq!(got.region.len(), 31, "a + 15 whole characters");
}

#[test]
fn a_long_call_says_the_longest_time_it_can() {
    let r = Recorder::holding(2);
    r.record(
        &Finished {
            took: Duration::from_secs(3 * 3600),
            ..call(1)
        },
        1,
    );
    assert_eq!(r.newest(1).calls[0].took_us, u32::MAX);
}

#[tokio::test]
async fn a_watcher_is_woken_by_a_call_and_by_nothing_else() {
    let r = Recorder::holding(4);
    let mut woken = r.watch();
    // Nothing recorded: the wait does not return (there is no clock in the recorder to return it).
    let quiet = tokio::time::timeout(Duration::from_millis(50), woken.changed()).await;
    assert!(quiet.is_err(), "woken with no call");
    r.record(&call(1), 1);
    tokio::time::timeout(Duration::from_secs(1), woken.changed())
        .await
        .expect("woken by the call")
        .expect("the recorder is alive");
    assert_eq!(*woken.borrow(), 1);
}

#[test]
fn recording_with_no_reader_is_fine() {
    let r = Recorder::holding(2);
    for n in 1..=5 {
        r.record(&call(n), n);
    }
    assert_eq!(r.newest_seq(), 5);
}
