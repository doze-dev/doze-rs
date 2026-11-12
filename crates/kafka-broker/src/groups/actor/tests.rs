//! What the group's keeper writes down, and when: a record is written when a group gets its first member or a longer
//! session, not for each member that joins.

use std::sync::Mutex;

use bytes::Bytes;
use kafka_meta::ConsumerLife;
use kafka_wire::Uuid;
use kafka_wire::ids::MemberId;

use super::super::consumer::{self, Heartbeat, TopicInfo, TopicSource, Topics};
use super::super::group::{Group, Settings};
use super::super::machine::{Event, IdSource, JoinRequest, Millis, Protocol, Token};
use super::{Keeper, Records};

#[derive(Default)]
struct Counting(Mutex<(u32, u32)>);

impl Records for Counting {
    fn live(&self, _: u32, _: &str, _: Option<ConsumerLife>) -> bool {
        self.0.lock().unwrap().0 += 1;
        true
    }
    fn empty(&self, _: &str, _: Option<ConsumerLife>, _: i64) -> bool {
        self.0.lock().unwrap().1 += 1;
        true
    }
}

struct One;

impl TopicSource for One {
    fn topics(&self) -> Topics {
        Topics::new([TopicInfo {
            name: "t".into(),
            id: Uuid::from_u128(1),
            partitions: 2,
        }])
    }
}

fn ids() -> IdSource {
    let mut n = 0;
    Box::new(move |_| {
        n += 1;
        MemberId::new(&format!("made-{n}")).unwrap()
    })
}

fn join(member: &str) -> Event {
    Event::Join(
        Token(0),
        JoinRequest {
            member_id: MemberId::new(member),
            instance: None,
            protocol_type: "consumer".into(),
            protocols: vec![Protocol {
                name: "range".into(),
                metadata: Bytes::from_static(&[0, 0, 0, 0, 0, 1, 0, 1, b't', 255, 255, 255, 255]),
            }],
            session_ms: 10_000,
            rebalance_ms: 30_000,
            client_id: "c".into(),
            client_host: "/127.0.0.1".into(),
            issued: true,
            id_first: false,
        },
    )
}

#[test]
fn a_record_is_written_for_a_groups_first_member_and_not_for_each_classic_member_that_joins_a_consumer_group()
 {
    let mut group = Group::classic(Settings {
        initial_delay_ms: 0,
        consumer: consumer::Config {
            session_ms: 45_000,
            assign_interval_ms: 0,
        },
        ids,
    });
    let mut keeper = Keeper {
        records: Counting::default(),
        session: 0,
        unwritten_empty: None,
        consumer: false,
    };
    let step = |keeper: &mut Keeper<Counting>, group: &mut Group, event: Event| {
        let had = group.member_count() > 0;
        assert!(keeper.may_seat(group, &event));
        let _ = group.step(event, Millis(1_000), &One);
        keeper.after(group, had);
    };
    step(&mut keeper, &mut group, join("a"));
    // A member of the new protocol upgrades the group: its record becomes the consumer group's, once.
    let heartbeat = Heartbeat {
        group: "g".into(),
        member_id: MemberId::new("n"),
        version: 1,
        epoch: 0,
        instance: None,
        rack: None,
        rebalance_ms: 60_000,
        subscribed: Some(vec!["t".to_owned()]),
        regex: None,
        assignor: None,
        owned: Some(Vec::new()),
        client_id: "consumer".into(),
        client_host: "/127.0.0.1".into(),
        rights: None,
    };
    step(
        &mut keeper,
        &mut group,
        Event::ConsumerHeartbeat(Token(0), heartbeat),
    );
    let written = keeper.records.0.lock().unwrap().0;
    // Classic members of the same session join the consumer group: nothing new is written for them.
    for i in 0..20 {
        step(&mut keeper, &mut group, join(&format!("c{i}")));
    }
    assert_eq!(keeper.records.0.lock().unwrap().0, written);
}
