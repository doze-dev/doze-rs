//! The peer against itself over a real (in-process) doze-os channel: one test per promise of protocol.md.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod common;

use common::{ALL, connected, handler, nothing};
use doze_module_proto::flow::WINDOW;
use doze_module_proto::method::Method;
use doze_module_proto::msg::console::{Row, TailArgs};
use doze_module_proto::msg::error::ErrorKind;
use doze_module_proto::msg::instance::Connection;
use doze_module_proto::msg::run::{Ensure, Ensured};
use doze_module_proto::peer::{CallOpts, Incoming};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

fn ensure() -> Ensure {
    Ensure {
        engine: "postgres".into(),
        full: "17.6.0".into(),
        target: "t".into(),
        expected_sha: None,
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_call_and_its_answer() {
    let module = handler(|c: Incoming| async move {
        let _ = c.responder.ok(&Connection {
            env_var: "ECHO_URL".into(),
            url: "echo://x".into(),
        });
    });
    let (host, _m) = connected(0, nothing(), module).await;
    let got: Connection = host
        .call(Method::Connection, &(), CallOpts::default())
        .await
        .unwrap();
    assert_eq!(got.env_var, "ECHO_URL");
}

/// `fetch.ensure` is allowed while the host's `resolve` is in flight and names it; not otherwise.
#[tokio::test(flavor = "current_thread")]
async fn a_callback_is_allowed_only_inside_the_call_it_names() {
    let host = handler(|c: Incoming| async move {
        let _ = c.responder.ok(&Ensured {
            bin_dir: "/cache/bin".into(),
            digest: "sha256:ab".into(),
        });
    });
    let module_slot: Arc<std::sync::Mutex<Option<doze_module_proto::peer::Peer>>> = Arc::default();
    let slot = module_slot.clone();
    let module = handler(move |c: Incoming| {
        let me = slot.lock().unwrap().clone().unwrap();
        async move {
            let opts = CallOpts {
                for_call: Some(c.id()),
                ..CallOpts::default()
            };
            let r: Result<Ensured, _> = me.call(Method::FetchEnsure, &ensure(), opts).await;
            let _ = c
                .responder
                .ok(&r.map(|e| e.digest).unwrap_or_else(|e| e.message));
        }
    });
    let (host_peer, m) = connected(0, host, module).await;
    *module_slot.lock().unwrap() = Some(m.clone());
    let digest: String = host_peer
        .call(Method::Resolve, &(), CallOpts::default())
        .await
        .unwrap();
    assert_eq!(digest, "sha256:ab");
    // Outside any resolve: refused by the host with a structured error.
    let opts = CallOpts {
        for_call: Some(9999),
        ..CallOpts::default()
    };
    let e = m
        .call::<_, Ensured>(Method::FetchEnsure, &ensure(), opts)
        .await
        .unwrap_err();
    assert_eq!(e.kind(), ErrorKind::Invalid, "{e}");
}

/// A deadline ends the caller's wait at once and cancels the callee's work.
#[tokio::test(flavor = "current_thread")]
async fn a_deadline_times_out_the_caller_and_cancels_the_callee() {
    let saw_cancel = Arc::new(AtomicU32::new(0));
    let s = saw_cancel.clone();
    let module = handler(move |c: Incoming| {
        let s = s.clone();
        async move {
            // Hold the whole call: capturing only `c.cancel` would drop the responder at once, and the peer would
            // answer "dropped without an answer" for us (the SDK hands handlers a context that keeps it alive).
            let call = c;
            call.cancel.cancelled().await;
            s.store(1, Ordering::SeqCst);
            drop(call);
        }
    });
    let (host, _m) = connected(0, nothing(), module).await;
    let t0 = Instant::now();
    let opts = CallOpts {
        deadline: Some(Duration::from_millis(200)),
        ..CallOpts::default()
    };
    let e = host
        .call::<_, ()>(Method::Plan, &(), opts)
        .await
        .unwrap_err();
    assert_eq!(e.kind(), ErrorKind::TimedOut);
    assert!(t0.elapsed() < Duration::from_millis(1000));
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        saw_cancel.load(Ordering::SeqCst),
        1,
        "the module never saw the cancel"
    );
}

/// A module that returns without answering does not leave the host waiting.
#[tokio::test(flavor = "current_thread")]
async fn a_dropped_call_is_answered_for_the_handler() {
    let (host, _m) = connected(0, nothing(), nothing()).await;
    let e = host
        .call::<_, ()>(Method::Plan, &(), CallOpts::default())
        .await
        .unwrap_err();
    assert_eq!(e.kind(), ErrorKind::Internal);
    assert!(e.message.contains("without an answer"), "{e}");
}

/// The host refuses, before sending, what the module did not declare.
#[tokio::test(flavor = "current_thread")]
async fn a_capability_the_module_did_not_declare_is_refused_locally() {
    let (host, _m) = connected(0, nothing(), nothing()).await;
    let e = host
        .stream(
            Method::Tail,
            &TailArgs {
                panel: "p".into(),
                selector: String::new(),
            },
            CallOpts::default(),
        )
        .unwrap_err();
    assert_eq!(e.kind(), ErrorKind::Unsupported);
}

/// A module streaming as fast as it can stays within one window of a slow reader, and every row arrives in order.
#[tokio::test(flavor = "current_thread")]
async fn a_stream_never_runs_more_than_a_window_ahead_of_its_reader() {
    let sent = Arc::new(AtomicU32::new(0));
    let s = sent.clone();
    let module = handler(move |c: Incoming| {
        let s = s.clone();
        async move {
            for i in 0..1000u32 {
                let row = Row {
                    cells: [("n".to_string(), i.to_string())].into(),
                    tone: String::new(),
                };
                if c.responder.item(&row).await.is_err() {
                    return;
                }
                s.fetch_add(1, Ordering::SeqCst);
            }
            let _ = c.responder.end();
        }
    });
    let (host, _m) = connected(ALL, nothing(), module).await;
    let mut stream = host
        .stream(
            Method::Tail,
            &TailArgs {
                panel: "p".into(),
                selector: String::new(),
            },
            CallOpts::default(),
        )
        .unwrap();
    let mut read = 0u32;
    while let Some(row) = stream.next::<Row>().await {
        assert_eq!(row.unwrap().cells["n"], read.to_string());
        read += 1;
        let ahead = sent.load(Ordering::SeqCst) - read;
        assert!(ahead <= WINDOW + 1, "the module ran {ahead} items ahead");
        if read.is_multiple_of(50) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }
    assert_eq!(read, 1000);
}
