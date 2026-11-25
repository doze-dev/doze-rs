//! The console's key browser and set-key form on the real valkey server (`DOZE_VALKEY_BINDIR`): the tree
//! of keys split on `:`, a filter, each kind of value, a big string a part at a time, and the form. The checks are
//! doze-resp's, shared with the other Redis-protocol module. Unix-only, like the engine.
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

#[path = "../../doze-resp/tests/support/engine_blocks.rs"]
mod engine_blocks;

use std::time::Duration;

#[tokio::test(flavor = "current_thread")]
async fn the_keys_open_as_a_tree_each_value_shows_as_what_it_is_and_the_form_sets_a_key() {
    let Some(h) =
        doze_module_test::acceptance(env!("CARGO_BIN_EXE_doze-module-valkey"), "valkey").unwrap()
    else {
        return;
    };
    let b = h
        .boot("valkey \"browse\" {\n  version = 9\n  password = \"pw\"\n}\n")
        .await
        .unwrap();
    engine_blocks::browse_and_set(&b, "pw").await;
    b.stop(Duration::from_secs(20)).await;
}
