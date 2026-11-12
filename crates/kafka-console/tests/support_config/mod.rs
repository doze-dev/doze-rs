//! Shared by the live tests of the Config pages: the topic they use, a world with it, and DescribeConfigs read back.
#![allow(
    dead_code,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::path::PathBuf;

use kafka_console::backend::requests::forms::topic_configs;
use kafka_wire::api::Response;

use crate::support_write::{World, world};

pub const TOPIC: &str = "inventory.levels";

pub fn out_dir() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest.parent().unwrap().parent().unwrap();
    let target =
        std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root.join("target"), PathBuf::from);
    target.join("board-diff-live")
}

pub async fn seeded(name: &str) -> World {
    let w = world(name, |_, _| {}).await;
    w.create_topic(
        TOPIC,
        1,
        &[
            ("cleanup.policy", "compact"),
            ("segment.bytes", "8388608"),
            ("min.compaction.lag.ms", "0"),
        ],
    )
    .await;
    w.create_topic("orders", 2, &[]).await;
    w
}

/// A config of the topic as DescribeConfigs says it: value and source number.
pub async fn described(w: &World, key: &str) -> Option<(String, i8)> {
    let Response::DescribeConfigs(r) = w.ask(4, topic_configs(TOPIC, &[key])).await else {
        panic!()
    };
    r.results[0]
        .configs
        .iter()
        .find(|c| c.name.as_str() == key)
        .map(|c| {
            (
                c.value
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_default(),
                c.config_source,
            )
        })
}
