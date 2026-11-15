//! Helpers the client and follow tests share.
#![allow(dead_code, clippy::unwrap_used, clippy::panic)]

use doze_control::fixture::{FixtureDaemon, Script, temp_socket};
use doze_control::{Frame, Stream, StreamEvent};
use std::io;
use std::path::Path;
use std::time::{Duration, Instant};

/// A Go fixture hub's script, as captured from Go.
pub(crate) fn script(hub: &str) -> Script {
    Script::load(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("testdata/go/scripts/{hub}.ndjson")),
    )
    .unwrap()
}

/// A fixture daemon playing `s` on a fresh socket; `None` (the test is skipped) where the system has no local
/// control socket yet (Windows, until the named pipe).
pub(crate) fn start_with(tag: &str, s: Script) -> Option<FixtureDaemon> {
    match FixtureDaemon::start(&temp_socket(tag), s) {
        Ok(d) => Some(d),
        Err(e) if e.kind() == io::ErrorKind::Unsupported => {
            eprintln!("skipped: {e}");
            None
        }
        Err(e) => panic!("{tag}: {e}"),
    }
}

/// A fixture daemon playing a Go hub.
pub(crate) fn start(hub: &str) -> Option<FixtureDaemon> {
    start_with(hub, script(hub))
}

/// The next topic frame; panics on an end, a loss, or five seconds of nothing.
pub(crate) fn frame(s: &Stream) -> Frame {
    match s.next_timeout(Duration::from_secs(5)) {
        Some(StreamEvent::Frame(f)) => *f,
        other => panic!("wanted a frame, got {other:?}"),
    }
}

/// Waits (up to five seconds) until `ok`.
pub(crate) fn until(what: &str, mut ok: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !ok() {
        assert!(Instant::now() < deadline, "timed out waiting until {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}
