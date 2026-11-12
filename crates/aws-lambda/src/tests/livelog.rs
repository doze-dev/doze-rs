//! What a function prints reaches its log group while it runs, not only when it ends, and the whole log is still `START`, the lines,
//! `END` and `REPORT`, each once.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde_json::json;

use super::sources::logged;
use super::{World, have};

#[tokio::test]
async fn a_line_is_in_the_log_before_the_invocation_ends() {
    if !have("python3") {
        return;
    }
    let w = World::new();
    w.create(
        "slow",
        "python3.12",
        "index.handler",
        &[(
            "index.py",
            b"import time\ndef handler(e, c):\n    print('first', flush=True)\n    time.sleep(1.5)\n    print('second', flush=True)\n    return 1\n",
            0o644,
        )],
    )
    .await;
    let done = AtomicBool::new(false);
    let invoke = async {
        w.invoke("slow", &json!({})).await;
        done.store(true, Ordering::SeqCst);
    };
    let watch = async {
        for _ in 0..100 {
            if logged(&w, "slow", "first").await.len() == 1 {
                return !done.load(Ordering::SeqCst);
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        false
    };
    let ((), seen_while_running) = tokio::join!(invoke, watch);
    assert!(
        seen_while_running,
        "the first line was written while the function ran"
    );
    let lines = logged(&w, "slow", "").await;
    assert_eq!(lines.len(), 5, "{lines:?}");
    assert!(lines[0].starts_with("START RequestId: "), "{lines:?}");
    assert_eq!(&lines[1..3], ["first", "second"]);
    assert!(lines[3].starts_with("END RequestId: ") && lines[4].starts_with("REPORT RequestId: "));
    w.stop().await;
}
