use std::time::Duration;

use aws_gateway::{Did, Finished};

use super::*;

fn call(n: u64) -> Finished<'static> {
    Finished {
        request_id: "0c6f3a52-7a8b-4c1d-9e2f-000000000000",
        signing_name: "sqs",
        sdk_id: "SQS",
        action: "SendMessage",
        member: "",
        resource: "",
        access_key_id: "test",
        region: "us-east-1",
        status: 200,
        code: "",
        took: Duration::from_micros(n),
        would_be_denied: false,
        user_agent: "",
        message: "",
        function_error: "",
        answer_bytes: None,
        did: Did::NONE,
        lab: None,
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
        user_agent: &long,
        message: &long,
        function_error: &long,
        answer_bytes: Some(70_000),
        lab: Some(aws_gateway::LabNote {
            rule: 3,
            name: &long,
            done: aws_gateway::Done::Failed,
        }),
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
    assert_eq!(one.bytes(), RECORD_BYTES + MOST_WIRE_TEXT);
    assert_eq!(one.resource.len(), MOST_RESOURCE);
    assert_eq!(one.caller.len(), MOST_CALLER);
    // The whole ring, at its worst, is at most a mebibyte (the Lab's tag took the last of the headroom: 16 bytes a row).
    const { assert!(MOST_BYTES <= 1 << 20) };
    // A slot grew from 160 to 184 bytes with the member that named the resource and the cause of a call between services, and
    // to 200 with the client its User-Agent named, and to 216 with the pointer to the words of an error or a failed function and the
    // length of the answer (16 KB more for a full ring; a success holds no more text than before).
    const { assert!(RECORD_BYTES <= 216) };
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

fn bus<'a>(acting: &'a str, cause: Option<&'a aws_peers::Cause>) -> aws_peers::BusCall<'a> {
    aws_peers::BusCall {
        acting,
        signing_name: "sqs",
        sdk_id: "SQS",
        operation: "SendMessage",
        member: "QueueUrl",
        resource: "http://localhost:4566/000000000000/orders",
        access_key_id: "test",
        region: "us-east-1",
        status: 200,
        code: "",
        took: Duration::from_micros(40),
        cause,
    }
}

#[test]
fn a_call_between_services_is_recorded_with_its_cause_and_counted_in_bytes() {
    let r = Recorder::holding(8);
    r.record(&call(1), 1);
    let cause = aws_peers::Cause {
        request: Some("req-9".into()),
        name: Some("subscription orders".into()),
    };
    let seq = r.record_bus(&bus("sns", Some(&cause)), 2);
    let made = r.get(seq).unwrap();
    assert_eq!(made.member, "QueueUrl");
    assert_eq!(made.request_id.len(), 0);
    let via = made.via.as_deref().unwrap();
    assert_eq!(
        (&*via.by, &*via.request, &*via.name),
        ("sns", "req-9", "subscription orders")
    );
    assert_eq!(
        r.fill().bytes,
        r.newest(8).calls.iter().map(Call::bytes).sum::<usize>()
    );
    assert!(made.bytes() <= RECORD_BYTES + MOST_TEXT);
    // A long cause is cut like any other text.
    let long = "n".repeat(500);
    let cause = aws_peers::Cause {
        request: None,
        name: Some(long.as_str().into()),
    };
    let seq = r.record_bus(&bus(&long, Some(&cause)), 3);
    let made = r.get(seq).unwrap();
    assert_eq!(made.via.as_deref().unwrap().name.len(), MOST_VIA);
    assert!(made.bytes() <= RECORD_BYTES + MOST_TEXT);
    assert_eq!(r.get(99), None);
}

#[test]
fn flows_count_each_caller_to_each_service_and_keep_the_most_recent() {
    let r = Recorder::holding(4);
    for n in 1..=3 {
        r.record(&call(n), n);
    }
    r.record(
        &Finished {
            status: 403,
            code: "AccessDenied",
            ..call(4)
        },
        4,
    );
    r.record_bus(&bus("sns", None), 5);
    r.record(
        &Finished {
            access_key_id: "other",
            would_be_denied: true,
            ..call(6)
        },
        6,
    );
    let (flows, dropped) = r.flows();
    assert_eq!(dropped, 0);
    let summary: Vec<_> = flows
        .iter()
        .map(|f| {
            (
                &*f.caller,
                f.internal,
                f.calls,
                f.outcomes,
                f.would_be_denied,
                f.last_seq,
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            ("other", false, 1, [1, 0, 0, 0], 1, 6),
            ("sns", true, 1, [1, 0, 0, 0], 0, 5),
            ("test", false, 4, [3, 0, 1, 0], 0, 4),
        ]
    );
    // Counts outlive the ring: four calls held, six counted.
    assert_eq!(r.fill().held, 4);
    // Past the most flows, the quietest goes.
    for n in 0..FLOWS {
        let key = format!("k{n}");
        r.record(
            &Finished {
                access_key_id: &key,
                ..call(1)
            },
            10,
        );
    }
    let (flows, dropped) = r.flows();
    assert_eq!(flows.len(), FLOWS);
    assert_eq!(dropped, 3);
    assert!(flows.iter().all(|f| f.caller.starts_with('k')));
}

#[test]
fn the_consoles_own_calls_are_let_go_before_an_apps_once_past_their_share() {
    let r = Recorder::holding(8);
    let console = |n| Finished {
        access_key_id: crate::resources::bus::CONSOLE,
        ..call(n)
    };
    // Two of the app's, then the console reading page after page.
    r.record(&call(1), 1);
    r.record(&call(2), 2);
    for n in 3..=20 {
        r.record(&console(n), n);
    }
    let held = r.newest(100);
    let callers: Vec<&str> = held.calls.iter().map(|c| &*c.caller).collect();
    // The app's two stay; the console keeps the rest of the ring (its share is the most it holds when an app's call is let go).
    assert_eq!(seqs(&held), [1, 2, 15, 16, 17, 18, 19, 20]);
    assert_eq!(callers[..2], ["test", "test"]);
    // Numbers have gaps now; a reader is given what is held after its number, and one call is found by its number.
    assert_eq!(seqs(&r.after(2, 3)), [15, 16, 17]);
    assert_eq!(seqs(&r.after(5, 100)), [15, 16, 17, 18, 19, 20]);
    assert_eq!(seqs(&r.newest(3)), [18, 19, 20]);
    assert_eq!(r.get(16).map(|c| c.seq), Some(16));
    assert!(r.get(5).is_none());
    // Within its share, the oldest call goes, whoever made it.
    let r = Recorder::holding(8);
    for n in 1..=8 {
        r.record(&call(n), n);
    }
    r.record(&console(9), 9);
    r.record(&call(10), 10);
    assert_eq!(seqs(&r.newest(100)), [3, 4, 5, 6, 7, 8, 9, 10]);
}

#[test]
fn a_workers_calls_outlast_three_times_the_rings_worth_of_page_reads() {
    // The ring is not what emptied Step Functions' Recent calls after ten minutes of browsing (the window a page reads was: see
    // `a_window_of_the_apps_calls_is_not_spent_on_the_consoles_own`). The console's reads past a quarter of the ring are let go first,
    // so a worker's calls stay in it however long the console is open. (Checked with the real binary: 1,500 reads, the worker's two
    // calls still held.)
    let r = Recorder::new();
    let console = |n| Finished {
        access_key_id: crate::resources::bus::CONSOLE,
        ..call(n)
    };
    r.record(&call(1), 1);
    r.record(&call(2), 2);
    for n in 3..(3 + 3 * CAPACITY as u64) {
        r.record(&console(n), n);
    }
    let held = r.newest(usize::MAX);
    assert_eq!(held.calls.len(), CAPACITY);
    assert_eq!(
        held.calls.iter().filter(|c| &*c.caller == "test").count(),
        2
    );
}

#[test]
fn a_window_of_the_apps_calls_is_not_spent_on_the_consoles_own() {
    // The pages that ask what the apps did read the newest 200 or so: with the console's reads in them (up to a quarter of the
    // ring), the window held nothing of a worker's, and the activity page said no one polled it.
    let r = Recorder::new();
    let console = |n| Finished {
        access_key_id: crate::resources::bus::CONSOLE,
        ..call(n)
    };
    r.record(&call(1), 1);
    r.record(&call(2), 2);
    for n in 3..1500 {
        r.record(&console(n), n);
    }
    let window = r.newest(200);
    assert_eq!(window.calls.len(), 200);
    assert!(
        window.calls.iter().all(|c| &*c.caller != "test"),
        "the console's reads fill the window"
    );
    let apps = r.newest_apps(200);
    assert_eq!(seqs(&apps), [1, 2]);
    assert_eq!(apps.last, r.newest_seq(), "a page that reached the end");
    assert!(!apps.more);
    assert_eq!(seqs(&r.newest_apps(1)), [2]);
}
