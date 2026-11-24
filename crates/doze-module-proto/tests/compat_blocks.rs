//! Old and new builds talking about the console's working blocks (protocol minor 2): a host that knows them never
//! asks a 2.1 module for one, whatever bits it set, and the new calls are minor 2 and gated by their own bits. The
//! messages read both ways are in tests/compat.rs.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod common;

use common::nothing;
use doze_module_proto::method::Method;
use doze_module_proto::msg::blocks::{self, Children, ChildrenArgs, ValueArgs};
use doze_module_proto::msg::error::ErrorKind;
use doze_module_proto::msg::hello::caps;
use doze_module_proto::peer;
use doze_os::channel;
use std::collections::BTreeMap;

/// A host that knows the blocks and a module that does not (minor 1): the calls are refused on the host's side, before
/// a frame is sent, even if the module set the bit (a bit a 2.1 build might mean something else by).
#[tokio::test(flavor = "current_thread")]
async fn a_2_1_module_is_never_asked_for_a_block() {
    for (minor, bits, why) in [
        (1, caps::VALUE | caps::TREE, "minor"),
        (2, caps::ADMIN, "does not offer"),
    ] {
        let (a, b) = channel::pair().await.unwrap();
        let host = tokio::spawn(peer::accept_module(a, nothing(), None));
        let (_m, _w) = peer::connect_host(b, common::hello(bits, minor), nothing())
            .await
            .unwrap();
        let (h, _) = host.await.unwrap().unwrap();
        let v = ValueArgs {
            instance: common::instance(),
            panel: "m".into(),
            row: BTreeMap::new(),
            offset: 0,
            max: 0,
        };
        let e = h
            .call::<_, blocks::ValueAnswer>(Method::Value, &v, peer::CallOpts::default())
            .await
            .unwrap_err();
        assert_eq!(e.kind(), ErrorKind::Unsupported, "{e}");
        assert!(e.message.contains(why), "{minor}: {e}");
        let c = ChildrenArgs {
            instance: common::instance(),
            panel: "t".into(),
            parent: String::new(),
            filter: String::new(),
            cursor: None,
        };
        let e = h
            .call::<_, Children>(Method::Children, &c, peer::CallOpts::default())
            .await
            .unwrap_err();
        assert_eq!(e.kind(), ErrorKind::Unsupported, "{e}");
    }
}

/// A module that knows the blocks and a host that does not: the host agrees its own, older minor, and the module's
/// new bits are kept and never acted on (above, a_newer_minor_module_is_accepted…). The other way: a 2.2 host's
/// block call reaching a 2.1 module is a method that module does not know, refused cleanly (as method 199 is).
#[test]
fn the_new_calls_are_minor_2_and_need_their_bits() {
    for (m, bit) in [(Method::Value, caps::VALUE), (Method::Children, caps::TREE)] {
        assert_eq!((m.since(), m.capability()), (2, bit), "{m:?}");
    }
    assert!(doze_module_proto::version::CURRENT.has(2));
    let bits = [
        caps::CONVERGE,
        caps::OBJECTS,
        caps::PRUNE,
        caps::ATTRIBUTES,
        caps::BACKEND_URL,
        caps::TEMPLATE,
        caps::RESTART,
        caps::HEALTH,
        caps::ADMIN,
        caps::CONSOLE,
        caps::WIRE,
        caps::EVENTS,
        caps::MIGRATE,
        caps::VALUE,
        caps::TREE,
    ];
    let all = bits.iter().fold(0u64, |acc, b| {
        assert_eq!(acc & b, 0, "bit {b:#x} used twice");
        acc | b
    });
    assert_eq!(all.count_ones(), 15);
}
