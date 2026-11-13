//! Old and new builds talking (protocol.md row 1). There is no newer build yet, so this test plays one: raw frames
//! from a "2.1" module (a manifest field, a callback and an error kind the current build does not know) and from a
//! "3.0" module, against the current host.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod common;

use common::nothing;
use doze_module_proto::frame::{Decoder, Frame, Kind, encode};
use doze_module_proto::method::Method;
use doze_module_proto::msg::console::{Action, ConsoleSpec};
use doze_module_proto::msg::error::{Error, ErrorKind};
use doze_module_proto::msg::hello::{Schema, Welcome, WireVersion};
use doze_module_proto::msg::{self, CallHead};
use doze_module_proto::peer;
use doze_os::channel::{self, Reader, Writer};
use minicbor::{Decode, Encode};

/// The manifest as a later minor might send it: every field of 2.0, then one more.
#[derive(Debug, Clone, Encode, Decode)]
struct Manifest21 {
    #[n(0)]
    engine: String,
    #[n(1)]
    category: String,
    #[n(2)]
    title: String,
    #[n(3)]
    tagline: String,
    #[n(4)]
    versions: Vec<String>,
    #[n(5)]
    exact_dots: u8,
    #[n(6)]
    default_port: u16,
    #[n(7)]
    boot_budget_ms: Option<u32>,
    #[n(8)]
    supervised: bool,
    #[n(9)]
    backend_socket: Option<String>,
    #[n(10)]
    caps: u64,
    #[n(11)]
    schema: Schema,
    #[n(12)]
    example: String,
    #[n(13)]
    console: Option<ConsoleSpec>,
    #[n(14)]
    actions: Vec<Action>,
    #[n(15)]
    targets: Vec<String>,
    #[n(16)]
    new_in_2_1: String,
}

#[derive(Debug, Clone, Encode, Decode)]
struct Hello21 {
    #[n(0)]
    version: WireVersion,
    #[n(1)]
    module: String,
    #[n(2)]
    release: String,
    #[n(3)]
    manifest: Manifest21,
}

fn hello21(major: u16) -> Hello21 {
    Hello21 {
        version: WireVersion { major, minor: 1 },
        module: "echo".into(),
        release: "9.9.9".into(),
        manifest: Manifest21 {
            engine: "echo".into(),
            category: "test".into(),
            title: "Echo".into(),
            tagline: String::new(),
            versions: vec![],
            exact_dots: 2,
            default_port: 7000,
            boot_budget_ms: None,
            supervised: false,
            backend_socket: None,
            caps: 1 << 40,
            schema: Schema::default(),
            example: String::new(),
            console: None,
            actions: vec![],
            targets: vec![],
            new_in_2_1: "a field 2.0 has never heard of".into(),
        },
    }
}

/// A raw module end: writes frames, reads the next one.
struct Raw {
    r: Reader,
    w: Writer,
    d: Decoder,
}

impl Raw {
    async fn send<T: Encode<()>>(&mut self, id: u32, method: u16, args: &T, head: CallHead) {
        let body = msg::encode_call(head, args).unwrap();
        self.w
            .write_all(&encode(id, Kind::Call, method, &body, None).unwrap())
            .await
            .unwrap();
    }
    async fn next(&mut self) -> Option<Frame> {
        let mut buf = vec![0u8; 64 << 10];
        loop {
            if let Some(f) = self.d.next_frame().unwrap() {
                return Some(f);
            }
            let n = self.r.read(&mut buf).await.unwrap();
            if n == 0 {
                return None;
            }
            self.d.push(&buf[..n]);
        }
    }
}

async fn raw_module() -> (
    tokio::task::JoinHandle<Result<(peer::Peer, msg::hello::Hello), Error>>,
    Raw,
) {
    let (a, b) = channel::pair().await.unwrap();
    let host = tokio::spawn(peer::accept_module(a, nothing(), None));
    let (r, w) = b.split();
    (
        host,
        Raw {
            r,
            w,
            d: Decoder::new(channel::TRAILER_LEN),
        },
    )
}

#[tokio::test(flavor = "current_thread")]
async fn a_newer_minor_module_is_accepted_and_its_unknown_callback_refused_cleanly() {
    let (host, mut m) = raw_module().await;
    m.send(1, Method::Hello as u16, &hello21(2), CallHead::default())
        .await;
    let reply = m.next().await.unwrap();
    assert_eq!(
        reply.kind,
        Kind::Reply,
        "{:?}",
        msg::decode::<Error>(&reply.body).ok()
    );
    let welcome: Welcome = msg::decode(&reply.body).unwrap();
    assert_eq!(
        (welcome.version.major, welcome.version.minor),
        (2, 0),
        "the older minor wins"
    );
    let (_host, seen) = host.await.unwrap().unwrap();
    assert_eq!(seen.manifest.engine, "echo");
    assert_eq!(
        seen.manifest.caps,
        1 << 40,
        "an unknown capability bit is kept, not refused"
    );

    // A callback added in 2.1 that this host does not know: a structured refusal, and the channel stays usable.
    m.send(2, 199, &(), CallHead::default()).await;
    let refused = m.next().await.unwrap();
    assert_eq!((refused.id, refused.kind), (2, Kind::Error));
    let e: Error = msg::decode(&refused.body).unwrap();
    assert_eq!(e.kind(), ErrorKind::Unsupported, "{e}");
    m.send(3, 199, &(), CallHead::default()).await;
    assert_eq!(
        m.next().await.unwrap().id,
        3,
        "the channel stopped answering after a refusal"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_module_of_another_major_is_told_who_should_update() {
    let (host, mut m) = raw_module().await;
    m.send(1, Method::Hello as u16, &hello21(3), CallHead::default())
        .await;
    let reply = m.next().await.unwrap();
    assert_eq!(reply.kind, Kind::Error);
    let e: Error = msg::decode(&reply.body).unwrap();
    assert!(e.message.contains("update doze"), "{e}");
    assert!(
        host.await.unwrap().is_err(),
        "the host accepted a module of another major"
    );
}

/// An error kind from a newer build reads as internal instead of failing to decode.
#[test]
fn an_unknown_error_kind_reads_as_internal() {
    #[derive(Encode)]
    enum Kind21 {
        #[n(7)]
        RateLimited,
    }
    #[derive(Encode)]
    struct Error21 {
        #[n(0)]
        kind: Option<Kind21>,
        #[n(1)]
        message: String,
    }
    let bytes = msg::encode(&Error21 {
        kind: Some(Kind21::RateLimited),
        message: "slow down".into(),
    })
    .unwrap();
    let e: Error = msg::decode(&bytes).unwrap();
    assert_eq!((e.kind, e.kind()), (None, ErrorKind::Internal));
    assert_eq!(e.message, "slow down");
}
