//! The client quota APIs and the controller mutation quota, step by step.

use kafka_protocol::messages::*;

use crate::pair::Pair;
use crate::requests::{
    Change, Entity, FilterCase, Op, alter, create, describe, norm_alter, norm_describe, topic,
};

pub(crate) fn client_quotas(pair: &mut Pair) {
    let alice: Entity<'_> = &[("user", Some("alice"))];
    let alice_c1: Entity<'_> = &[("user", Some("alice")), ("client-id", Some("c1"))];
    let default_user: Entity<'_> = &[("user", None)];
    let default_user_c1: Entity<'_> = &[("user", None), ("client-id", Some("c1"))];
    let default_client: Entity<'_> = &[("client-id", None)];
    let ip: Entity<'_> = &[("ip", Some("127.0.0.1"))];
    let set: &[Change<'_>] = &[
        (
            alice,
            &[
                ("producer_byte_rate", Some(1024.0)),
                ("consumer_byte_rate", Some(2048.0)),
            ],
        ),
        (alice_c1, &[("request_percentage", Some(12.5))]),
        (default_user, &[("controller_mutation_rate", Some(3.0))]),
        (default_user_c1, &[("producer_byte_rate", Some(4096.0))]),
        (default_client, &[("consumer_byte_rate", Some(100.0))]),
        (ip, &[("connection_creation_rate", Some(50.0))]),
    ];
    for version in 0..=1 {
        pair.same::<_, AlterClientQuotasResponse>(
            "validate only",
            ApiKey::AlterClientQuotas,
            version,
            &alter(set, true),
            norm_alter,
        );
    }
    pair.same::<_, DescribeClientQuotasResponse>(
        "nothing set yet",
        ApiKey::DescribeClientQuotas,
        1,
        &describe(&[], false),
        norm_describe,
    );
    pair.same::<_, AlterClientQuotasResponse>(
        "set",
        ApiKey::AlterClientQuotas,
        1,
        &alter(set, false),
        norm_alter,
    );
    pair.settle_quotas();
    let refused: &[Change<'_>] = &[
        (&[], &[("producer_byte_rate", Some(1.0))]),
        (
            &[("group", Some("g"))],
            &[("producer_byte_rate", Some(1.0))],
        ),
        (&[("user", Some(""))], &[("producer_byte_rate", Some(1.0))]),
        (
            &[("ip", Some("127.0.0.1")), ("user", Some("x"))],
            &[("connection_creation_rate", Some(1.0))],
        ),
        (
            &[("ip", Some("no-such-host.invalid"))],
            &[("connection_creation_rate", Some(1.0))],
        ),
        (&[("user", Some("b1"))], &[("bogus", Some(1.0))]),
        (
            &[("user", Some("b2"))],
            &[("producer_byte_rate", Some(0.0))],
        ),
        (
            &[("user", Some("b3"))],
            &[("producer_byte_rate", Some(-5.0))],
        ),
        (
            &[("user", Some("b4"))],
            &[("producer_byte_rate", Some(1.5))],
        ),
        (
            &[("user", Some("b5"))],
            &[("consumer_byte_rate", Some(1e19))],
        ),
        (
            &[("user", Some("b6"))],
            &[("producer_byte_rate", Some(f64::INFINITY))],
        ),
        (
            &[("user", Some("b7"))],
            &[
                ("producer_byte_rate", Some(1.0)),
                ("producer_byte_rate", Some(2.0)),
            ],
        ),
        (&[("ip", None)], &[("connection_creation_rate", Some(3e9))]),
        (&[("ip", None)], &[("producer_byte_rate", Some(1.0))]),
        (
            &[("client-id", Some("c9"))],
            &[("connection_creation_rate", Some(1.0))],
        ),
        (
            &[("user", Some("b8"))],
            &[("producer_byte_rate", Some(10.0))],
        ),
        (
            &[("user", Some("b8"))],
            &[("consumer_byte_rate", Some(10.0))],
        ),
        (&[("user", Some("b9"))], &[("producer_byte_rate", None)]),
    ];
    for version in 0..=1 {
        pair.same::<_, AlterClientQuotasResponse>(
            "refusals",
            ApiKey::AlterClientQuotas,
            version,
            &alter(refused, true),
            norm_alter,
        );
    }
    pair.same::<_, AlterClientQuotasResponse>(
        "refusals, for real",
        ApiKey::AlterClientQuotas,
        1,
        &alter(refused, false),
        norm_alter,
    );
    pair.settle_quotas();
    let filters: &[FilterCase<'_>] = &[
        ("everything", &[], false),
        ("strict nothing", &[], true),
        ("alice", &[("user", 0, Some("alice"))], false),
        ("alice strict", &[("user", 0, Some("alice"))], true),
        ("default user", &[("user", 1, None)], false),
        ("default user strict", &[("user", 1, None)], true),
        ("any user", &[("user", 2, None)], false),
        (
            "any user and client",
            &[("user", 2, None), ("client-id", 2, None)],
            true,
        ),
        ("client c1", &[("client-id", 0, Some("c1"))], false),
        ("default client", &[("client-id", 1, None)], true),
        ("ips", &[("ip", 2, None)], false),
        ("an ip", &[("ip", 0, Some("127.0.0.1"))], true),
        ("nobody", &[("user", 0, Some("nobody"))], false),
        ("empty type", &[("", 0, Some("x"))], false),
        ("twice", &[("user", 2, None), ("user", 0, Some("a"))], false),
        ("unknown type", &[("group", 0, Some("x"))], false),
        ("exact without a name", &[("user", 0, None)], false),
        ("default with a name", &[("user", 1, Some("a"))], false),
        ("specified with a name", &[("user", 2, Some("a"))], false),
        ("unknown match", &[("user", 9, None)], false),
        ("ip and user", &[("ip", 2, None), ("user", 2, None)], false),
    ];
    for (name, components, strict) in filters {
        for version in 0..=1 {
            pair.same::<_, DescribeClientQuotasResponse>(
                name,
                ApiKey::DescribeClientQuotas,
                version,
                &describe(components, *strict),
                norm_describe,
            );
        }
    }
    // Everything removed again: removing the last key of an entity removes the entity.
    let clear: Vec<(Entity<'_>, Vec<Op<'_>>)> = set
        .iter()
        .map(|(e, ops)| (*e, ops.iter().map(|(k, _)| (*k, None)).collect()))
        .collect();
    let clear: Vec<Change<'_>> = clear.iter().map(|(e, ops)| (*e, ops.as_slice())).collect();
    pair.same::<_, AlterClientQuotasResponse>(
        "clear",
        ApiKey::AlterClientQuotas,
        1,
        &alter(&clear, false),
        norm_alter,
    );
    let b8: &[Change<'_>] = &[(
        &[("user", Some("b8"))],
        &[("producer_byte_rate", None), ("consumer_byte_rate", None)],
    )];
    pair.same::<_, AlterClientQuotasResponse>(
        "clear b8",
        ApiKey::AlterClientQuotas,
        1,
        &alter(b8, false),
        norm_alter,
    );
    pair.settle_quotas();
    pair.same::<_, DescribeClientQuotasResponse>(
        "all gone",
        ApiKey::DescribeClientQuotas,
        1,
        &describe(&[], false),
        norm_describe,
    );
}

/// `controller_mutation_rate`: a bucket of 11 seconds' worth; the topic that empties it is made, the next refused.
pub(crate) fn mutation_quota(pair: &mut Pair, kafka: &str, doze: &str) {
    let rate: &[Change<'_>] = &[(
        &[("client-id", Some("oracle-diff"))],
        &[("controller_mutation_rate", Some(1.0))],
    )];
    let _: (AlterClientQuotasResponse, AlterClientQuotasResponse) =
        pair.both(ApiKey::AlterClientQuotas, 1, &alter(rate, false));
    pair.settle_quotas();
    let topics = [
        ("oracle-m1", 5),
        ("oracle-m2", 5),
        ("oracle-m3", 5),
        ("oracle-m4", 5),
    ];
    let norm = |r: &mut CreateTopicsResponse| {
        r.throttle_time_ms = r.throttle_time_ms.signum();
        for t in &mut r.topics {
            t.topic_id = kafka_wire::Uuid::nil();
            t.configs = None;
        }
    };
    pair.same::<_, CreateTopicsResponse>(
        "mutations: strict",
        ApiKey::CreateTopics,
        7,
        &create(&topics),
        norm,
    );
    // Each next step on new connections: the ones above are muted for the throttle time, which would refill the bucket.
    let delete = |name: &str| {
        let mut req = DeleteTopicsRequest::default();
        req.timeout_ms = 30_000;
        req.topic_names.push(topic(name));
        req
    };
    let norm_delete = |r: &mut DeleteTopicsResponse| {
        r.throttle_time_ms = r.throttle_time_ms.signum();
        for t in &mut r.responses {
            t.topic_id = kafka_wire::Uuid::nil();
        }
    };
    pair.reconnect(kafka, doze);
    pair.same::<_, DeleteTopicsResponse>(
        "mutations: strict delete",
        ApiKey::DeleteTopics,
        5,
        &delete("oracle-m1"),
        norm_delete,
    );
    pair.reconnect(kafka, doze);
    pair.same::<_, DeleteTopicsResponse>(
        "mutations: permissive delete",
        ApiKey::DeleteTopics,
        4,
        &delete("oracle-m1"),
        norm_delete,
    );
    let grow = |name: &str, count: i32| {
        let mut req = CreatePartitionsRequest::default();
        req.timeout_ms = 30_000;
        let mut t =
            kafka_protocol::messages::create_partitions_request::CreatePartitionsTopic::default();
        t.name = topic(name);
        t.count = count;
        req.topics.push(t);
        req
    };
    let norm_grow =
        |r: &mut CreatePartitionsResponse| r.throttle_time_ms = r.throttle_time_ms.signum();
    pair.reconnect(kafka, doze);
    pair.same::<_, CreatePartitionsResponse>(
        "mutations: strict grow",
        ApiKey::CreatePartitions,
        3,
        &grow("oracle-m2", 7),
        norm_grow,
    );
    pair.reconnect(kafka, doze);
    pair.same::<_, CreatePartitionsResponse>(
        "mutations: permissive grow",
        ApiKey::CreatePartitions,
        2,
        &grow("oracle-m2", 7),
        norm_grow,
    );
    pair.reconnect(kafka, doze);
    pair.same::<_, CreateTopicsResponse>(
        "mutations: permissive create",
        ApiKey::CreateTopics,
        5,
        &create(&[("oracle-m5", 2)]),
        norm,
    );
    pair.reconnect(kafka, doze);
    let unset: &[Change<'_>] = &[(
        &[("client-id", Some("oracle-diff"))],
        &[("controller_mutation_rate", None)],
    )];
    let _: (AlterClientQuotasResponse, AlterClientQuotasResponse) =
        pair.both(ApiKey::AlterClientQuotas, 1, &alter(unset, false));
}
