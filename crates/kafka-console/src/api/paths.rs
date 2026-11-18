//! The address of every GET endpoint, for the tests that read them all (the planted-secret test among them).

/// Every GET endpoint, with the queries worth reading.
pub const GET_PATHS: &[&str] = &[
    "/api/v1/home",
    "/api/v1/connect",
    "/api/v1/shell",
    "/api/v1/jump",
    "/api/v1/jump?q=or",
    // Slice R1b: the topic writes and the Config tab (the worlds of the tests have `orders`).
    "/api/v1/create-topic",
    "/api/v1/create-topic?step=review&name=fresh&partitions=2",
    "/api/v1/topics/orders/produce",
    "/api/v1/topics/orders/produce?key=k&value=v&hn0=a&hv0=b",
    "/api/v1/topics/orders/add-partitions",
    "/api/v1/topics/orders/delete-records?partition=0",
    "/api/v1/topics/orders/delete",
    "/api/v1/topics/orders/config",
    "/api/v1/topics/orders/config/edit?key=retention.ms",
    "/api/v1/topics/orders/config/edit?key=cleanup.policy",
    "/api/v1/topics/orders/config/edit?key=min.compaction.lag.ms",
    "/api/v1/topics/orders/config/reset?key=retention.ms",
    // Groups (R2): the addresses that answer whatever groups the broker has (those of one group are
    // `groups::GROUP_GET_PATHS`, over the fixtures' groups).
    "/api/v1/groups",
    "/api/v1/groups?q=pay&state=stable&state=empty",
    // Live (R3). The tail (`/api/v1/tail/live`) is a stream that never ends: its own tests read it.
    "/api/v1/live",
    "/api/v1/live?refused=1&idle=0&client=orders&api=Produce&topic=orders",
    "/api/v1/live?lab=all",
    "/api/v1/live/clients",
    "/api/v1/live/first",
    "/api/v1/live/topic-wire?topic=orders",
    "/api/v1/live/group-wire?group=fulfilment",
    // R1a: the Topics pages. Messages and a record are not here: showing a record's key, value and headers is their job.
    "/api/v1/topics",
    "/api/v1/topics?q=or&internal=1",
    "/api/v1/topics/orders",
    "/api/v1/topics/orders/consumers",
    "/api/v1/topics/orders/log",
    "/api/v1/topics/orders/producers",
    "/api/v1/settings",
    "/api/v1/settings/edit?key=log.retention.ms",
    "/api/v1/access",
    "/api/v1/access?state=secured",
    "/api/v1/access/acls/new?type=topic&name=orders&principal=User:orders",
    "/api/v1/access/acls/delete?type=topic&name=orders&pattern=literal&principal=User:orders&host=*&op=write&perm=allow",
    // The Lab's other GETs answer LAB_OFF where the Lab is not installed (the fixtures, a broker with --lab=false); their tests
    // are in api/lab_tests.rs and the browser gate.
    "/api/v1/lab",
    "/api/v1/lab/guide",
    // 10d: the list says why there is no registry; the rest are read over one in tests/live_api_schemas*.rs.
    "/api/v1/schemas",
    // The Data page and what Empty topic will remove (docs/port/data-control.md).
    "/api/v1/data",
    "/api/v1/data/empty?topic=orders",
];
