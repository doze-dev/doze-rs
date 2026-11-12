use std::sync::Arc;

use kafka_wire::messages::ApiKey;

use super::*;
use crate::recorder::row::Who;

fn row(api: ApiKey, f: impl FnOnce(&mut Facts)) -> Row {
    let mut facts = Facts::default();
    f(&mut facts);
    Row {
        id: 1,
        touched: 1,
        first_ms: 0,
        last_ms: 0,
        count: 1,
        conn: 1,
        who: Arc::new(Who {
            client: "c".into(),
            software: None,
            version: None,
            principal: "p".into(),
            peer: None,
        }),
        api,
        version: 9,
        correlation: 1,
        facts: Box::new(facts),
        class: Class::Ok,
        closed: None,
        code: 0,
        request_bytes: 1_203_441,
        response_bytes: 0,
        tooks: vec![14_000],
        idle: false,
        fold: 0,
    }
}

#[test]
fn a_target_is_the_partition_the_group_or_the_transaction() {
    let r = row(ApiKey::Produce, |f| f.topic("orders", [0]));
    assert_eq!(target(&r), ("orders-0".into(), None));
    let r = row(ApiKey::Produce, |f| f.topic("clickstream", 0..6));
    assert_eq!(
        target(&r),
        ("clickstream".into(), Some("6 partitions".into()))
    );
    let r = row(ApiKey::Fetch, |f| {
        f.topic("payments.settled", [1]);
        f.from_offset = Some(7402);
    });
    assert_eq!(
        target(&r),
        ("payments.settled-1".into(), Some("from 7,402".into()))
    );
    let r = row(ApiKey::JoinGroup, |f| {
        f.group("fulfilment");
        f.generation = Some(30);
    });
    assert_eq!(
        target(&r),
        ("fulfilment".into(), Some("generation 30".into()))
    );
    let r = row(ApiKey::LeaveGroup, |f| {
        f.group("fulfilment");
        f.member("fulfilment-py-2");
    });
    assert_eq!(
        target(&r),
        ("fulfilment".into(), Some("by fulfilment-py-2".into()))
    );
    let r = row(ApiKey::EndTxn, |f| f.txn("orders-eos"));
    assert_eq!(target(&r).0, "orders-eos");
    assert_eq!(
        target(&row(ApiKey::Metadata, |f| f.about("all topics"))).0,
        "all topics"
    );
    assert_eq!(target(&row(ApiKey::ListGroups, |_| {})).0, "-");
}

#[test]
fn what_an_answer_adds_is_counted_in_the_boards_words() {
    assert_eq!(
        done(&row(ApiKey::Produce, |f| f.records = 1)).as_deref(),
        Some("1 record")
    );
    assert_eq!(
        done(&row(ApiKey::Produce, |f| f.records = 9640)).as_deref(),
        Some("9,640 records")
    );
    let r = row(ApiKey::Fetch, |f| {
        f.batches = 1;
        f.records = 3;
        f.bytes = 211;
    });
    assert_eq!(done(&r).as_deref(), Some("1 batch, 211 B"));
    assert_eq!(
        done(&row(ApiKey::Fetch, |_| {})).as_deref(),
        Some("caught up, no records")
    );
    assert_eq!(
        done(&row(ApiKey::Metadata, |f| f.items = 9)).as_deref(),
        Some("9 topics")
    );
    let mut folded = row(ApiKey::Produce, |f| f.records = 14_832);
    folded.count = 412;
    assert_eq!(done(&folded).as_deref(), Some("about 36 records each"));
}

#[test]
fn a_refusal_says_the_brokers_words_then_a_size_then_our_sentence() {
    let mut r = row(ApiKey::Produce, |f| {
        f.say(Some("Compacted topic requires every record to have a key"))
    });
    r.code = 87;
    assert_eq!(
        refused(&r),
        "Compacted topic requires every record to have a key"
    );
    let mut r = row(ApiKey::Produce, |_| {});
    r.code = 10;
    assert_eq!(refused(&r), "a request of 1.1 MiB");
    let mut r = row(ApiKey::Heartbeat, |_| {});
    r.code = 27;
    assert!(refused(&r).contains("changing members"));
}

#[test]
fn words_for_each_class_and_the_took_line() {
    assert_eq!(word(Class::Denied, false), "Denied");
    assert_eq!(word(Class::Refused, false), "Refused");
    assert_eq!(word(Class::Ok, true), "Closed");
    let mut r = row(ApiKey::Fetch, |f| f.wait_ms = Some(480));
    assert_eq!(took_sub(&r).as_deref(), Some("willing to wait 480 ms"));
    r.count = 3;
    assert_eq!(took_sub(&r).as_deref(), Some("median"));
}
