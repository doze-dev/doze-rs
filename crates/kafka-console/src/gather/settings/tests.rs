use std::time::Duration;

use kafka_wire::messages::ApiKey;
use kafka_wire::profile::Advertised;

use super::*;
use crate::model::SettingsFlag;

fn ints() -> Intervals {
    Intervals {
        retention_check: Duration::from_secs(300),
        cleaner: Duration::from_secs(15),
        cleaner_on: true,
        fsync: Duration::from_millis(500),
        offsets_retention: Duration::from_secs(7 * 24 * 3600),
        group_initial_rebalance: Duration::from_millis(200),
        consumer_session: Duration::from_secs(45),
        transactions: Duration::from_millis(500),
        node_id: 1,
    }
}

#[test]
fn the_intervals_are_the_flags_values_written_as_the_flags_write_them_with_their_defaults() {
    let none = intervals(&ints(), &StartFacts::default(), (None, None));
    let got: Vec<(&str, &str, &str)> = none
        .rows
        .iter()
        .map(|r| (r.flag, r.value.as_str(), r.default.as_str()))
        .collect();
    assert_eq!(
        got,
        [
            ("--log-retention-check-interval", "5m", "unknown"),
            ("--log-cleaner-interval", "15s", "unknown"),
            ("--fsync-interval", "500ms", "unknown"),
            ("--offsets-retention", "168h", "unknown"),
            ("--group-initial-rebalance-delay", "200ms", "unknown"),
            ("--consumer-session-timeout", "45s", "unknown"),
        ]
    );
    assert_eq!(
        (
            none.transactions.as_str(),
            none.retention_every.as_str(),
            none.cleaner_every.as_str()
        ),
        ("500 ms", "5 m", "15 s")
    );
    let known = StartFacts {
        flags: vec![SettingsFlag {
            name: "--fsync-interval".into(),
            group: "g".into(),
            value: "90ms".into(),
            default: "500ms".into(),
            set: true,
            what: String::new(),
            dynamic: None,
        }],
        ..StartFacts::default()
    };
    let with = intervals(&ints(), &known, (None, None));
    assert_eq!(with.rows[2].default, "500ms");
}

#[test]
fn offsets_retention_says_how_long_that_is() {
    assert_eq!(offsets_note(&ints(), None), "kept for 7 d");
    let soonest = ("analytics".to_owned(), "3 d 2 h".to_owned());
    assert_eq!(
        offsets_note(&ints(), Some(&soonest)),
        "kept for 7 d; analytics' expire in 3 d 2 h"
    );
    let own = ("orders-api".to_owned(), "1 d".to_owned());
    assert_eq!(
        offsets_note(&ints(), Some(&own)),
        "kept for 7 d; orders-api's expire in 1 d"
    );
    let mut forever = ints();
    forever.offsets_retention = Duration::ZERO;
    assert_eq!(offsets_note(&forever, None), "kept for ever");
}

#[test]
fn apis_are_listed_by_family_and_the_board_order_not_by_key() {
    let list = [
        Advertised {
            key: ApiKey::Metadata,
            min: 0,
            max: 12,
        },
        Advertised {
            key: ApiKey::Fetch,
            min: 4,
            max: 13,
        },
        Advertised {
            key: ApiKey::Produce,
            min: 3,
            max: 9,
        },
        Advertised {
            key: ApiKey::ApiVersions,
            min: 0,
            max: 4,
        },
        Advertised {
            key: ApiKey::OffsetDelete,
            min: 0,
            max: 0,
        },
    ];
    let rows = api_rows(&list);
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "Produce",
            "Fetch",
            "ApiVersions",
            "Metadata",
            "OffsetDelete"
        ]
    );
    assert_eq!(rows[0].versions, "v3 to v9");
    assert_eq!(rows[4].versions, "v0 only");
    assert_eq!(rows[2].family, "Metadata and bootstrapping");
}
