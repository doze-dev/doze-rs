//! The builder against real shapes: what a service writes comes out as the JSON a client expects, and reads back through
//! the same keys.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use aws_model_dynamodb::keys::{AttributeValue as A, GetItemOutput as G};
use aws_model_sqs::keys::{
    ListQueuesResult as LQ, Message as M, ReceiveMessageResult as RM, SendMessageResult as S,
};
use aws_schema::arena::Arena;
use aws_schema::build::Object;
use aws_schema::enc::encode;
use aws_schema::json::decode;
use aws_schema::patterns::Patterns;
use aws_schema::view::{Shape, View};

fn text(t: &aws_schema::tables::Tables, shape: u32, ar: &Arena, root: u32) -> String {
    let mut out = Vec::new();
    encode(t, shape, ar, "", root, &mut out).unwrap();
    String::from_utf8(out).unwrap()
}

#[test]
fn a_flat_answer_is_written_in_the_order_it_was_built() {
    let t = &aws_model_sqs::tables::TABLES;
    let mut ar = Arena::new();
    let mut out = Object::<S>::root(&mut ar);
    out.put(S::MESSAGE_ID, "id-1")
        .put(S::MD5_OF_MESSAGE_BODY, "9b2cf535f27731c974343645a3985328");
    let root = out.finish();
    assert_eq!(
        text(t, S::INDEX, &ar, root),
        r#"{"MessageId":"id-1","MD5OfMessageBody":"9b2cf535f27731c974343645a3985328"}"#
    );
}

#[test]
fn lists_maps_and_nested_structures() {
    let t = &aws_model_sqs::tables::TABLES;
    let mut ar = Arena::new();
    let mut out = Object::<RM>::root(&mut ar);
    out.put_list(RM::MESSAGES, |messages| {
        messages.push(|m| {
            m.put(M::MESSAGE_ID, "m1").put(M::BODY, "he said \"hi\"\n");
            m.put_map(M::ATTRIBUTES, |attrs| {
                attrs.insert("SentTimestamp", "1700000000000");
            });
        });
        messages.push(|m| {
            m.put(M::MESSAGE_ID, "m2");
        });
    });
    let root = out.finish();
    assert_eq!(
        text(t, RM::INDEX, &ar, root),
        r#"{"Messages":[{"MessageId":"m1","Body":"he said \"hi\"\n","Attributes":{"SentTimestamp":"1700000000000"}},{"MessageId":"m2"}]}"#
    );
}

#[test]
fn an_empty_list_is_an_empty_list_not_an_absent_member() {
    let t = &aws_model_sqs::tables::TABLES;
    let mut ar = Arena::new();
    let mut out = Object::<LQ>::root(&mut ar);
    out.put_list(LQ::QUEUE_URLS, |_| {});
    let root = out.finish();
    assert_eq!(text(t, LQ::INDEX, &ar, root), r#"{"QueueUrls":[]}"#);
}

#[test]
fn what_was_built_reads_back_through_the_same_keys() {
    let t = &aws_model_dynamodb::tables::TABLES;
    let mut ar = Arena::new();
    let mut out = Object::<G>::root(&mut ar);
    out.put_map(G::ITEM, |item| {
        item.insert("a", |v| {
            v.put(A::S, "x");
        });
        item.insert("b", |v| {
            v.put_list(A::L, |l| {
                l.push(|e| {
                    e.put(A::N, "1");
                });
                l.push(|e| {
                    e.put_bool(A::BOOL, true);
                });
            });
        });
        item.insert("c", |v| {
            v.put_list(A::BS, |l| {
                l.push(b"\x00\xff");
            });
        });
    });
    let root = out.finish();
    let json = text(t, G::INDEX, &ar, root);
    assert_eq!(
        json,
        r#"{"Item":{"a":{"S":"x"},"b":{"L":[{"N":"1"},{"BOOL":true}]},"c":{"BS":["AP8="]}}}"#
    );

    // The same text decodes against the same shape and the keys find what was put.
    let mut back = Arena::new();
    let pats = Patterns::new(t);
    let root = decode(t, &pats, G::INDEX, &json, &mut back).unwrap();
    let view = View::<G>::root(&back, &json, root).unwrap();
    let b = view
        .get(G::ITEM)
        .unwrap()
        .find(|(k, _)| *k == "b")
        .unwrap()
        .1;
    assert_eq!(b.get(A::L).unwrap().count(), 2);
    let c = view
        .get(G::ITEM)
        .unwrap()
        .find(|(k, _)| *k == "c")
        .unwrap()
        .1;
    assert_eq!(c.get(A::BS).unwrap().next(), Some(&b"\x00\xff"[..]));
}
