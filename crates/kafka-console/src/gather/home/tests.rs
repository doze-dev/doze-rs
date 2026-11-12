use super::*;
use crate::fixtures;

#[test]
fn the_state_follows_the_data_and_attention_wins() {
    let a = fixtures::attention();
    assert_eq!(choose(0, None), HomeState::Quiet);
    assert_eq!(choose(3, None), HomeState::Steady);
    assert_eq!(choose(3, Some(a.clone())), HomeState::Attention(a.clone()));
    // A transaction needs a topic to be on, but the rule does not second-guess the broker.
    assert_eq!(choose(0, Some(a.clone())), HomeState::Attention(a));
}

#[test]
fn an_instance_card_counts_the_run_from_its_start() {
    let facts = crate::facts::Facts {
        listen: "127.0.0.1:9092".parse().unwrap(),
        advertised_host: "127.0.0.1".into(),
        advertised_port: 9092,
        tls: false,
        sasl: vec![],
        kafka_major: 4,
        allow_hosts: vec![],
        unprotected: false,
    };
    let i = Instance {
        started_ms: 1_793_988_127_000,
        last_stop: LastStop::Clean,
        open_partitions: 21,
        max_open_partitions: 64,
        max_open_files: 0,
        data_dir: "/no/such/dir".into(),
        acls: 0,
        acl_default_allow: true,
        stored_users: 0,
        process: kafka_broker::Process::default(),
    };
    let now = i.started_ms + (3 * 86_400 + 4 * 3600) * 1000;
    let v = instance_view(&i, &facts, fixtures::access_open(), now);
    assert_eq!(v.started, "Nov 6 18:02 UTC");
    assert_eq!(v.up_secs, 3 * 86_400 + 4 * 3600);
    assert_eq!(v.last_stop, StopView::Clean);
    assert_eq!(v.profile, "Kafka 4");
    assert_eq!(v.data_dir, "/no/such/dir");
    assert_eq!(v.open_files, None);
    // A clock that is behind the start never makes a negative age.
    assert_eq!(
        instance_view(&i, &facts, fixtures::access_open(), 0).up_secs,
        0
    );
}
