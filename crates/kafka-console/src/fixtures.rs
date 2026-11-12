//! The "larder" world of the design boards (`kit/world.md`) as view-models, so a page can be rendered, looked at and
//! compared with its board with no broker. Used by the tests and by the `boards` example (feature `examples`).
//! The headline numbers are checked against the boards' text by `tests/boards.rs`.

use crate::model::{
    Access, Attention, ConnectView, HeldBack, HomeState, HomeView, InstanceView, ProbeView, Rule,
    Span, StopView, Verdict,
};
use crate::snippets;

/// `127.0.0.1:9092`.
pub const ADDRESS: &str = "127.0.0.1:9092";

/// The access of the default world: open.
#[must_use]
pub fn access_open() -> Access {
    Access {
        label: "Open".to_owned(),
        sasl: false,
        detail: "No sign-in, no ACLs, plain TCP".to_owned(),
    }
}

fn ok_rule(title: &str, detail: Vec<Span>, value: &str) -> Rule {
    Rule {
        ok: true,
        title: title.to_owned(),
        detail,
        value: value.to_owned(),
    }
}

/// The advertised listener of the default world: three rules that pass.
#[must_use]
pub fn probe_ok() -> ProbeView {
    ProbeView {
        verdict: Verdict::Reachable,
        rules: vec![
            ok_rule(
                "The listener answers",
                vec![Span::text(format!(
                    "Dialled {ADDRESS} and asked ApiVersions"
                ))],
                "2 ms",
            ),
            ok_rule(
                "It says where clients should reconnect",
                vec![
                    Span::text("Metadata names this broker at "),
                    Span::mono(ADDRESS),
                    Span::text(": the advertised host and port equal the listener"),
                ],
                "Metadata",
            ),
            ok_rule(
                "That address is reachable",
                vec![Span::text(
                    "Dialled the advertised address back and got an answer",
                )],
                "2 ms",
            ),
        ],
        listen: ADDRESS.to_owned(),
        advertised: ADDRESS.to_owned(),
        same: true,
        metadata_version: 12,
    }
}

/// The instance card of the default world (Nov 9, 22:14, up 3 d 4 h).
#[must_use]
pub fn instance() -> InstanceView {
    InstanceView {
        started: "Nov 6 18:02 UTC".to_owned(),
        up_secs: 3 * 86_400 + 4 * 3600 + 12 * 60,
        last_stop: StopView::Clean,
        open_partitions: 21,
        max_open_partitions: 64,
        open_files: Some(118),
        resident_bytes: Some(4_718_592),
        threads: Some(3),
        profile: "Kafka 4".to_owned(),
        access: access_open(),
        data_dir: "~/larder/data/kafka".to_owned(),
    }
}

/// The transaction of the attention board: `orders-eos`, 41 s into 60 s.
#[must_use]
pub fn attention() -> Attention {
    Attention {
        count: 1,
        transactional_id: "orders-eos".to_owned(),
        producer_id: 4017,
        producer_epoch: 2,
        partition: "orders-0".to_owned(),
        topic: "orders".to_owned(),
        since: "22:13:33 UTC".to_owned(),
        timeout_secs: 60,
        elapsed_secs: 41,
        held_back: Some(HeldBack {
            last_stable: 16_098,
            high_watermark: 16_104,
        }),
    }
}

/// Home in one of its three states.
#[must_use]
pub fn home(state: HomeState) -> HomeView {
    let mut instance = instance();
    let (topics, partitions) = match state {
        HomeState::Quiet => {
            instance.up_secs = 120;
            instance.last_stop = StopView::None;
            instance.started = "Nov 9 22:12 UTC".to_owned();
            instance.open_partitions = 0;
            instance.open_files = None;
            instance.resident_bytes = Some(3_250_000);
            (0, 0)
        }
        _ => (9, 21),
    };
    let spoke = (state != HomeState::Quiet).then(crate::fixtures_live::spoke);
    HomeView {
        state,
        listen: ADDRESS.to_owned(),
        loopback: true,
        topics,
        partitions,
        probe: probe_ok(),
        instance,
        unchecked: 0,
        spoke,
    }
}

/// Connect for the default world.
#[must_use]
pub fn connect() -> ConnectView {
    ConnectView {
        listen: ADDRESS.to_owned(),
        loopback: true,
        probe: probe_ok(),
        access: access_open(),
        snippets: snippets::for_address(ADDRESS),
    }
}

/// The top bar of the default world.
#[must_use]
pub fn chrome() -> crate::ui::shell::Chrome {
    crate::ui::shell::Chrome {
        address: ADDRESS.to_owned(),
        profile: "Kafka 4 profile".to_owned(),
        security: "Open".to_owned(),
        locked: false,
        unprotected: false,
    }
}
