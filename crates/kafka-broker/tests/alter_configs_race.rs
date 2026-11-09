//! Two clients changing different configs of the same resource at once: each change is acknowledged, so each is kept.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use std::sync::Arc;

use support::{BROKER, TOPIC_RESOURCE as TOPIC, open_on, rig, topic};

const SET: i8 = 0;
const ROUNDS: usize = 200;
const DYNAMIC_TOPIC: i8 = 1;
const DYNAMIC_DEFAULT: i8 = 3;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_clients_changing_different_configs_of_one_topic_keep_both_changes() {
    let rig = Arc::new(rig());
    for i in 0..ROUNDS {
        let name = format!("t{i}");
        rig.create(vec![topic(&name, 1)], false).await;
        let (a, b) = (Arc::clone(&rig), Arc::clone(&rig));
        let (n1, n2) = (name.clone(), name.clone());
        let one = tokio::spawn(async move {
            let ops = vec![("retention.ms", SET, Some("111111"))];
            a.incremental(0, false, &[(TOPIC, n1.as_str(), ops)]).await
        });
        let two = tokio::spawn(async move {
            let ops = vec![("segment.bytes", SET, Some("2222222"))];
            b.incremental(0, false, &[(TOPIC, n2.as_str(), ops)]).await
        });
        let (one, two) = (one.await.unwrap(), two.await.unwrap());
        assert_eq!((one[0].0, two[0].0), (0, 0));
        for key in ["retention.ms", "segment.bytes"] {
            let got = rig.config_of(TOPIC, &name, key).await.unwrap();
            assert_eq!(
                got.config_source, DYNAMIC_TOPIC,
                "round {i}: the change to {key} was acknowledged and then lost"
            );
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_clients_changing_different_configs_of_the_broker_keep_both_changes_here_and_after_a_restart() {
    for i in 0..ROUNDS {
        let rig = Arc::new(rig());
        let (a, b) = (Arc::clone(&rig), Arc::clone(&rig));
        let one = tokio::spawn(async move {
            let ops = vec![("log.retention.bytes", SET, Some("9000"))];
            a.incremental(0, false, &[(BROKER, "", ops)]).await
        });
        let two = tokio::spawn(async move {
            let ops = vec![("log.retention.ms", SET, Some("5000"))];
            b.incremental(0, false, &[(BROKER, "", ops)]).await
        });
        let (one, two) = (one.await.unwrap(), two.await.unwrap());
        assert_eq!((one[0].0, two[0].0), (0, 0));
        let again = open_on(rig.fs.process_restart());
        for (who, label) in [(&*rig, "now"), (&again, "after a restart")] {
            for key in ["log.retention.bytes", "log.retention.ms"] {
                let got = who.config_of(BROKER, "", key).await;
                assert_eq!(
                    got.map(|c| c.config_source),
                    Some(DYNAMIC_DEFAULT),
                    "round {i}, {label}: the change to {key} was acknowledged and then lost"
                );
            }
        }
    }
}
